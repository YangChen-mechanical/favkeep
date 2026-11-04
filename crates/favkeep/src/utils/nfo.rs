use anyhow::Result;
use chrono::NaiveDateTime;
use favkeep_entity::upper_vec::Upper as EntityUpper;
use favkeep_entity::*;
use quick_xml::Error;
use quick_xml::events::{BytesCData, BytesText};
use quick_xml::writer::Writer;
use tokio::io::{AsyncWriteExt, BufWriter};

use crate::config::NFOTimeType;

#[allow(clippy::upper_case_acronyms)]
pub enum NFO<'a> {
    Movie(Movie<'a>),
    TVShow(TVShow<'a>),
    Upper(Upper),
    Episode(Episode<'a>),
}

pub struct Movie<'a> {
    pub name: &'a str,
    pub intro: &'a str,
    pub bvid: &'a str,
    pub uppers: Vec<EntityUpper<i64, &'a str>>,
    pub premiered: NaiveDateTime,
    pub tags: Option<Vec<String>>,
}

pub struct TVShow<'a> {
    pub name: &'a str,
    pub intro: &'a str,
    pub bvid: &'a str,
    pub uppers: Vec<EntityUpper<i64, &'a str>>,
    pub premiered: NaiveDateTime,
    pub tags: Option<Vec<String>>,
}

pub struct Upper {
    pub upper_id: String,
    pub pubtime: NaiveDateTime,
}

pub struct Episode<'a> {
    pub name: &'a str,
    pub pid: String,
}

impl NFO<'_> {
    pub async fn generate_nfo(self) -> Result<String> {
        let mut buffer = r#"<?xml version="1.0" encoding="utf-8" standalone="yes"?>
"#
        .as_bytes()
        .to_vec();
        let mut tokio_buffer = BufWriter::new(&mut buffer);
        let writer = Writer::new_with_indent(&mut tokio_buffer, b' ', 4);
        match self {
            NFO::Movie(movie) => {
                Self::write_movie_nfo(writer, movie).await?;
            }
            NFO::TVShow(tvshow) => {
                Self::write_tvshow_nfo(writer, tvshow).await?;
            }
            NFO::Upper(upper) => {
                Self::write_upper_nfo(writer, upper).await?;
            }
            NFO::Episode(episode) => {
                Self::write_episode_nfo(writer, episode).await?;
            }
        }
        tokio_buffer.flush().await?;
        Ok(String::from_utf8(buffer)?)
    }

    async fn write_movie_nfo(mut writer: Writer<&mut BufWriter<&mut Vec<u8>>>, movie: Movie<'_>) -> Result<()> {
        writer
            .create_element("movie")
            .write_inner_content_async::<_, _, Error>(|writer| async move {
                writer
                    .create_element("plot")
                    .write_cdata_content_async(BytesCData::new(Self::format_plot(movie.bvid, movie.intro)))
                    .await?;
                writer.create_element("outline").write_empty_async().await?;
                writer
                    .create_element("title")
                    .write_text_content_async(BytesText::new(movie.name))
                    .await?;
                for upper in movie.uppers {
                    writer
                        .create_element("actor")
                        .write_inner_content_async::<_, _, Error>(|writer| async move {
                            writer
                                .create_element("name")
                                .write_text_content_async(BytesText::new(&upper.mid.to_string()))
                                .await?;
                            writer
                                .create_element("role")
                                .write_text_content_async(BytesText::new(upper.role().as_ref()))
                                .await?;
                            writer
                                .create_element("thumb")
                                .write_text_content_async(BytesText::new(upper.face))
                                .await?;
                            Ok(writer)
                        })
                        .await?;
                }
                writer
                    .create_element("year")
                    .write_text_content_async(BytesText::new(&movie.premiered.format("%Y").to_string()))
                    .await?;
                if let Some(tags) = movie.tags {
                    for tag in tags {
                        writer
                            .create_element("genre")
                            .write_text_content_async(BytesText::new(&tag))
                            .await?;
                    }
                }
                writer
                    .create_element("uniqueid")
                    .with_attribute(("type", "bilibili"))
                    .write_text_content_async(BytesText::new(movie.bvid))
                    .await?;
                writer
                    .create_element("premiered")
                    .write_text_content_async(BytesText::new(&movie.premiered.format("%Y-%m-%d").to_string()))
                    .await?;
                Ok(writer)
            })
            .await?;
        Ok(())
    }

    async fn write_tvshow_nfo(mut writer: Writer<&mut BufWriter<&mut Vec<u8>>>, tvshow: TVShow<'_>) -> Result<()> {
        writer
            .create_element("tvshow")
            .write_inner_content_async::<_, _, Error>(|writer| async move {
                writer
                    .create_element("plot")
                    .write_cdata_content_async(BytesCData::new(Self::format_plot(tvshow.bvid, tvshow.intro)))
                    .await?;
                writer.create_element("outline").write_empty_async().await?;
                writer
                    .create_element("title")
                    .write_text_content_async(BytesText::new(tvshow.name))
                    .await?;
                for upper in tvshow.uppers {
                    writer
                        .create_element("actor")
                        .write_inner_content_async::<_, _, Error>(|writer| async move {
                            writer
                                .create_element("name")
                                .write_text_content_async(BytesText::new(&upper.mid.to_string()))
                                .await?;
                            writer
                                .create_element("role")
                                .write_text_content_async(BytesText::new(upper.role().as_ref()))
                                .await?;
                            writer
                                .create_element("thumb")
                                .write_text_content_async(BytesText::new(upper.face))
                                .await?;
                            Ok(writer)
                        })
                        .await?;
                }
                writer
                    .create_element("year")
                    .write_text_content_async(BytesText::new(&tvshow.premiered.format("%Y").to_string()))
                    .await?;
                if let Some(tags) = tvshow.tags {
                    for tag in tags {
                        writer
                            .create_element("genre")
                            .write_text_content_async(BytesText::new(&tag))
                            .await?;
                    }
                }
                writer
                    .create_element("uniqueid")
                    .with_attribute(("type", "bilibili"))
                    .write_text_content_async(BytesText::new(tvshow.bvid))
                    .await?;
                writer
                    .create_element("premiered")
                    .write_text_content_async(BytesText::new(&tvshow.premiered.format("%Y-%m-%d").to_string()))
                    .await?;
                Ok(writer)
            })
            .await?;
        Ok(())
    }

    async fn write_upper_nfo(mut writer: Writer<&mut BufWriter<&mut Vec<u8>>>, upper: Upper) -> Result<()> {
        writer
            .create_element("person")
            .write_inner_content_async::<_, _, Error>(|writer| async move {
                writer.create_element("plot").write_empty_async().await?;
                writer.create_element("outline").write_empty_async().await?;
                writer
                    .create_element("lockdata")
                    .write_text_content_async(BytesText::new("false"))
                    .await?;
                writer
                    .create_element("dateadded")
                    .write_text_content_async(BytesText::new(&upper.pubtime.format("%Y-%m-%d %H:%M:%S").to_string()))
                    .await?;
                writer
                    .create_element("title")
                    .write_text_content_async(BytesText::new(&upper.upper_id))
                    .await?;
                writer
                    .create_element("sorttitle")
                    .write_text_content_async(BytesText::new(&upper.upper_id))
                    .await?;
                Ok(writer)
            })
            .await?;
        Ok(())
    }

    async fn write_episode_nfo(mut writer: Writer<&mut BufWriter<&mut Vec<u8>>>, episode: Episode<'_>) -> Result<()> {
        writer
            .create_element("episodedetails")
            .write_inner_content_async::<_, _, Error>(|writer| async move {
                writer.create_element("plot").write_empty_async().await?;
                writer.create_element("outline").write_empty_async().await?;
                writer
                    .create_element("title")
                    .write_text_content_async(BytesText::new(episode.name))
                    .await?;
                writer
                    .create_element("season")
                    .write_text_content_async(BytesText::new("1"))
                    .await?;
                writer
                    .create_element("episode")
                    .write_text_content_async(BytesText::new(&episode.pid))
                    .await?;
                Ok(writer)
            })
            .await?;
        Ok(())
    }

    #[inline]
    fn format_plot(bvid: &str, intro: &str) -> String {
        format!(
            r#"原始视频：<a href="https://www.bilibili.com/video/{}/">{}</a><br/><br/>{}"#,
            bvid, bvid, intro,
        )
    }
}

pub trait ToNFO<'a, T> {
    fn to_nfo(&'a self, nfo_time_type: NFOTimeType) -> T;
}

impl<'a> ToNFO<'a, Movie<'a>> for &'a video::Model {
    fn to_nfo(&'a self, nfo_time_type: NFOTimeType) -> Movie<'a> {
        Movie {
            name: &self.name,
            intro: &self.intro,
            bvid: self.bvid.as_deref().unwrap_or_default(),
            uppers: self.uppers().collect(),
            premiered: match nfo_time_type {
                NFOTimeType::FavTime => self.favtime,
                NFOTimeType::PubTime => self.pubtime,
            },
            tags: self.tags.as_ref().map(|tags| tags.clone().into()),
        }
    }
}

impl<'a> ToNFO<'a, TVShow<'a>> for &'a video::Model {
    fn to_nfo(&'a self, nfo_time_type: NFOTimeType) -> TVShow<'a> {
        TVShow {
            name: &self.name,
            intro: &self.intro,
            bvid: self.bvid.as_deref().unwrap_or_default(),
            uppers: self.uppers().collect(),
            premiered: match nfo_time_type {
                NFOTimeType::FavTime => self.favtime,
                NFOTimeType::PubTime => self.pubtime,
            },
            tags: self.tags.as_ref().map(|tags| tags.clone().into()),
        }
    }
}

impl<'a> ToNFO<'a, Upper> for (&video::Model, &EntityUpper<i64, &str>) {
    fn to_nfo(&'a self, _nfo_time_type: NFOTimeType) -> Upper {
        Upper {
            upper_id: self.1.mid.to_string(),
            pubtime: self.0.pubtime,
        }
    }
}

impl<'a> ToNFO<'a, Episode<'a>> for &'a page::Model {
    fn to_nfo(&'a self, _nfo_time_type: NFOTimeType) -> Episode<'a> {
        Episode {
            name: &self.name,
            pid: self.pid.to_string(),
        }
    }
}

#[cfg(test)]
mod tests;
