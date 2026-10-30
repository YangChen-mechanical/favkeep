use core::str;
use std::io::SeekFrom;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, bail, ensure};
use async_tempfile::TempFile;
use futures::TryStreamExt;
use reqwest::{Method, StatusCode, header};
use tokio::fs::{self};
use tokio::io::{AsyncSeekExt, AsyncWrite, AsyncWriteExt, BufWriter};
use tokio::process::Command;
use tokio::task::JoinSet;
use tokio_util::io::StreamReader;

use crate::bilibili::{Client, ErrorForStatusExt};
use crate::config::{ARGS, ConcurrentDownloadLimit};

pub struct Downloader {
    client: Client,
}

pub(crate) async fn copy_response(resp: reqwest::Response, file: &mut (impl AsyncWrite + Unpin)) -> Result<()> {
    let expected = resp.header_content_length();
    let mut reader = StreamReader::new(resp.bytes_stream().map_err(std::io::Error::other));
    let mut writer = BufWriter::with_capacity(2 * 1024 * 1024, file);
    let received = tokio::io::copy(&mut reader, &mut writer).await?;
    writer.flush().await?;
    if let Some(expected) = expected {
        ensure!(
            received == expected,
            "downloaded bytes mismatch: expected {expected}, got {received}"
        );
    }
    Ok(())
}

impl Downloader {
    // Downloader 使用带有默认 Header 的 Client 构建
    // 拿到 url 后下载文件不需要任何 cookie 作为身份凭证
    // 但如果不设置默认 Header，下载时会遇到 403 Forbidden 错误
    pub fn new(client: Client) -> Self {
        Self { client }
    }

    pub async fn fetch(&self, url: &str, path: &Path, concurrent_download: &ConcurrentDownloadLimit) -> Result<()> {
        let mut temp_file = TempFile::new().await?;
        self.fetch_internal(url, &mut temp_file, false, concurrent_download)
            .await?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::copy(temp_file.file_path(), path).await?;
        // temp_file 的 drop 需要 std::fs::remove_file
        // 如果交由 rust 自动执行虽然逻辑正确但会略微阻塞异步上下文
        // 尽量主动调用，保证正常执行的情况下文件清除操作由 spawn_blocking 在专门线程中完成
        temp_file.drop_async().await;
        Ok(())
    }

    pub async fn multi_fetch(
        &self,
        urls: &[&str],
        path: &Path,
        concurrent_download: &ConcurrentDownloadLimit,
    ) -> Result<()> {
        let temp_file = self.multi_fetch_internal(urls, true, concurrent_download).await?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::copy(temp_file.file_path(), path).await?;
        temp_file.drop_async().await;
        Ok(())
    }

    pub async fn multi_fetch_and_merge(
        &self,
        video_urls: &[&str],
        audio_urls: &[&str],
        path: &Path,
        concurrent_download: &ConcurrentDownloadLimit,
    ) -> Result<()> {
        let (video_temp_file, audio_temp_file) = tokio::try_join!(
            self.multi_fetch_internal(video_urls, true, concurrent_download),
            self.multi_fetch_internal(audio_urls, true, concurrent_download)
        )?;
        let final_temp_file = TempFile::new().await?;
        let _process = crate::resources::acquire(&crate::resources::MEDIA_PROCESSES, "媒体处理").await;
        let output = Command::new(ARGS.ffmpeg_path.as_deref().unwrap_or("ffmpeg"))
            .args([
                "-i",
                video_temp_file.file_path().to_string_lossy().as_ref(),
                "-i",
                audio_temp_file.file_path().to_string_lossy().as_ref(),
                "-c",
                "copy",
                "-strict",
                "unofficial",
                "-f",
                "mp4",
                "-y",
                final_temp_file.file_path().to_string_lossy().as_ref(),
            ])
            .kill_on_drop(true)
            .output()
            .await
            .context("failed to run ffmpeg")?;
        drop(_process);
        if !output.status.success() {
            bail!("ffmpeg error: {}", str::from_utf8(&output.stderr).unwrap_or("unknown"));
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::copy(final_temp_file.file_path(), path).await?;
        tokio::join!(
            video_temp_file.drop_async(),
            audio_temp_file.drop_async(),
            final_temp_file.drop_async()
        );
        Ok(())
    }

    async fn multi_fetch_internal(
        &self,
        urls: &[&str],
        is_stream: bool,
        concurrent_download: &ConcurrentDownloadLimit,
    ) -> Result<TempFile> {
        if urls.is_empty() {
            bail!("no urls provided");
        }
        let mut temp_file = TempFile::new().await?;
        for (idx, url) in urls.iter().enumerate() {
            match self
                .fetch_internal(url, &mut temp_file, is_stream, concurrent_download)
                .await
            {
                Ok(_) => return Ok(temp_file),
                Err(e) => {
                    if idx == urls.len() - 1 {
                        temp_file.drop_async().await;
                        return Err(e).with_context(|| format!("failed to download file from all {} urls", urls.len()));
                    }
                    temp_file.set_len(0).await?;
                    temp_file.rewind().await?;
                }
            }
        }
        unreachable!()
    }

    async fn fetch_internal(
        &self,
        url: &str,
        file: &mut TempFile,
        is_stream: bool,
        concurrent_download: &ConcurrentDownloadLimit,
    ) -> Result<()> {
        if concurrent_download.enable {
            self.fetch_parallel(url, file, is_stream, concurrent_download).await
        } else {
            self.fetch_serial(url, file).await
        }
    }

    async fn fetch_serial(&self, url: &str, file: &mut TempFile) -> Result<()> {
        let _transfer = crate::resources::acquire(&crate::resources::TRANSFERS, "文件传输").await;
        let resp = self
            .client
            .request(Method::GET, url, None)
            .send()
            .await?
            .error_for_status_ext()?;
        copy_response(resp, file).await
    }

    async fn fetch_parallel(
        &self,
        url: &str,
        file: &mut TempFile,
        is_stream: bool,
        concurrent_download: &ConcurrentDownloadLimit,
    ) -> Result<()> {
        let (concurrency, threshold) = (concurrent_download.concurrency, concurrent_download.threshold);
        let probe = crate::resources::acquire(&crate::resources::TRANSFERS, "文件探测").await;
        let file_size = if is_stream {
            // B 站视频、音频流存在 HEAD 为 404 但 GET 正常的情况，此处假设支持分块，直接使用携带 Range 头的 GET 请求探测
            let resp = self
                .client
                .request(Method::GET, url, None)
                .header(header::RANGE, "bytes=0-0")
                .send()
                .await?
                .error_for_status_ext()?;
            if resp.status() != StatusCode::PARTIAL_CONTENT {
                drop(resp);
                drop(probe);
                return self.fetch_serial(url, file).await;
            }
            resp.header_file_size()
        } else {
            // 对于普通文件，直接使用常规的 HEAD 请求探测
            let resp = self
                .client
                .request(Method::HEAD, url, None)
                .send()
                .await?
                .error_for_status_ext()?;
            if resp
                .headers()
                .get(header::ACCEPT_RANGES)
                // https://developer.mozilla.org/en-US/docs/Web/HTTP/Reference/Headers/Accept-Ranges#none
                .is_none_or(|v| v.to_str().unwrap_or_default() == "none")
            {
                drop(resp);
                drop(probe);
                return self.fetch_serial(url, file).await;
            }
            resp.header_content_length()
        };
        drop(probe);
        let Some(file_size) = file_size else {
            return self.fetch_serial(url, file).await;
        };
        let chunk_size = file_size / concurrency as u64;
        if chunk_size < threshold {
            return self.fetch_serial(url, file).await;
        }
        file.set_len(file_size).await?;
        let mut tasks = JoinSet::new();
        let url = Arc::new(url.to_string());
        for i in 0..concurrency {
            let start = i as u64 * chunk_size;
            let end = if i == concurrency - 1 {
                file_size
            } else {
                start + chunk_size
            } - 1;
            let (url_clone, client_clone) = (url.clone(), self.client.clone());
            let mut file_clone = file.open_rw().await?;
            tasks.spawn(async move {
                let _transfer = crate::resources::acquire(&crate::resources::TRANSFERS, "分块传输").await;
                file_clone.seek(SeekFrom::Start(start)).await?;
                let range_header = format!("bytes={}-{}", start, end);
                let resp = client_clone
                    .request(Method::GET, &url_clone, None)
                    .header(header::RANGE, &range_header)
                    .send()
                    .await?
                    .error_for_status_ext()?;
                if let Some(content_length) = resp.header_content_length() {
                    ensure!(
                        content_length == end - start + 1,
                        "content length mismatch: expected {}, got {}",
                        end - start + 1,
                        content_length
                    );
                }
                let mut stream_reader = StreamReader::new(resp.bytes_stream().map_err(std::io::Error::other));
                let mut file_writer = BufWriter::with_capacity(2 * 1024 * 1024, file_clone);
                let received = tokio::io::copy(&mut stream_reader, &mut file_writer).await?;
                file_writer.flush().await?;
                ensure!(
                    received == end - start + 1,
                    "downloaded bytes mismatch: expected {}, got {}",
                    end - start + 1,
                    received,
                );
                Ok(())
            });
        }
        while let Some(res) = tasks.join_next().await {
            res??;
        }
        Ok(())
    }
}

/// reqwest.content_length() 居然指的是 body_size 而非 content-length header，没办法自己实现一下
/// https://github.com/seanmonstar/reqwest/issues/1814
trait ResponseExt {
    /// 获取 Content-Length 头的值
    fn header_content_length(&self) -> Option<u64>;
    /// 获取 Content-Range 头中的文件总大小部分
    fn header_file_size(&self) -> Option<u64>;
}

impl ResponseExt for reqwest::Response {
    fn header_content_length(&self) -> Option<u64> {
        self.headers()
            .get(header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
    }

    fn header_file_size(&self) -> Option<u64> {
        self.headers()
            .get(header::CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| {
                // Content-Range: bytes 0-0/800946
                s.rsplit_once('/')
            })
            .and_then(|(_, size_str)| size_str.parse::<u64>().ok())
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use anyhow::Result;

    use crate::bilibili::{BestStream, BiliClient, Video};
    use crate::config::VersionedConfig;
    use crate::database::setup_database;
    use crate::downloader::Downloader;

    #[tokio::test]
    async fn shared_transfer_limit_covers_chunks_and_serial_fallback() {
        use axum::{
            Router,
            body::Body,
            extract::State,
            http::{HeaderMap, StatusCode, Uri},
            routing::get,
        };
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        struct Active(Arc<AtomicUsize>);
        impl Drop for Active {
            fn drop(&mut self) {
                self.0.fetch_sub(1, Ordering::SeqCst);
            }
        }
        async fn serve(
            State((active, peak)): State<(Arc<AtomicUsize>, Arc<AtomicUsize>)>,
            uri: Uri,
            headers: HeaderMap,
        ) -> (StatusCode, HeaderMap, Body) {
            let data = b"0123456789abcdef";
            let mut response_headers = HeaderMap::new();
            let (status, bytes) = if uri.path() == "/range" {
                response_headers.insert("accept-ranges", "bytes".parse().unwrap());
                if let Some(range) = headers.get("range") {
                    let (start, end) = range
                        .to_str()
                        .unwrap()
                        .strip_prefix("bytes=")
                        .unwrap()
                        .split_once('-')
                        .unwrap();
                    let (start, end) = (start.parse::<usize>().unwrap(), end.parse::<usize>().unwrap());
                    response_headers.insert(
                        "content-range",
                        format!("bytes {start}-{end}/{}", data.len()).parse().unwrap(),
                    );
                    (StatusCode::PARTIAL_CONTENT, data[start..=end].to_vec())
                } else {
                    (StatusCode::OK, data.to_vec())
                }
            } else {
                (StatusCode::OK, data.to_vec())
            };
            response_headers.insert("content-length", bytes.len().to_string().parse().unwrap());
            let stream = futures::stream::once(async move {
                let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(current, Ordering::SeqCst);
                let _guard = Active(active);
                tokio::time::sleep(std::time::Duration::from_millis(30)).await;
                Ok::<_, std::io::Error>(bytes)
            });
            (status, response_headers, Body::from_stream(stream))
        }
        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let app = Router::new()
            .route("/range", get(serve))
            .route("/serial", get(serve))
            .with_state((active.clone(), peak.clone()));
        let server = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        }));
        let downloader = Downloader::new(crate::bilibili::Client::new());
        let config = crate::config::ConcurrentDownloadLimit {
            enable: true,
            concurrency: 4,
            threshold: 0,
        };
        let mut first = async_tempfile::TempFile::new().await.unwrap();
        let mut second = async_tempfile::TempFile::new().await.unwrap();
        let first_url = format!("{origin}/range");
        let second_url = format!("{origin}/serial");
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            tokio::try_join!(
                downloader.fetch_internal(&first_url, &mut first, true, &config),
                downloader.fetch_internal(&second_url, &mut second, true, &config)
            )
        })
        .await
        .unwrap()
        .unwrap();
        assert_eq!(tokio::fs::read(first.file_path()).await.unwrap(), b"0123456789abcdef");
        assert_eq!(tokio::fs::read(second.file_path()).await.unwrap(), b"0123456789abcdef");
        assert!(peak.load(Ordering::SeqCst) <= 2);
        assert_eq!(active.load(Ordering::SeqCst), 0);
        first.drop_async().await;
        second.drop_async().await;
        server.abort();
    }

    #[ignore = "only for manual test"]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_parse_and_download_video() -> Result<()> {
        VersionedConfig::init_for_test(&setup_database(Path::new("./test.sqlite")).await?).await?;
        let config = VersionedConfig::get().read();
        let client = BiliClient::new();
        let video = Video::new(&client, "BV1QJmaYKEv4", &config.credential);
        let pages = video.get_pages().await.expect("failed to get pages");
        let first_page = pages.into_iter().next().expect("no page found");
        let mut page_analyzer = video
            .get_page_analyzer(&first_page)
            .await
            .expect("failed to get page analyzer");
        let json_info = serde_json::to_string_pretty(&page_analyzer.info)?;
        tokio::fs::write("./debug_playurl.json", json_info).await?;
        let best_stream = page_analyzer
            .best_stream(&config.filter_option)
            .expect("failed to get best stream");
        let BestStream::VideoAudio {
            video,
            audio: Some(audio),
        } = best_stream
        else {
            panic!("best stream is not video & audio");
        };
        dbg!(&video);
        dbg!(&audio);
        let downloader = Downloader::new(client.client);
        downloader
            .multi_fetch_and_merge(
                &video.urls(true),
                &audio.urls(true),
                Path::new("./output.mp4"),
                &config.concurrent_limit.download,
            )
            .await
            .expect("failed to download video");
        Ok(())
    }
}
