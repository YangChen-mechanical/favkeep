use super::*;
use axum::Extension;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use bili_sync_entity::{page, video, watch_progress};
use bili_sync_migration::{Migrator, MigratorTrait};
use sea_orm::{ActiveModelTrait, ActiveValue::Set, DatabaseConnection, EntityTrait};
use sea_orm::{ConnectionTrait, Database};
use serde_json::{Value, json};
use tower::ServiceExt;

async fn list(db: &DatabaseConnection, params: &str) -> Value {
    let response = router()
        .layer(Extension(db.clone()))
        .oneshot(
            Request::builder()
                .uri(format!("/videos?{params}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice::<Value>(&bytes).unwrap()["data"].take()
}

#[tokio::test]
async fn mixed_platform_states_and_playability_filter_before_pagination() {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    Migrator::up(&db, None).await.unwrap();
    for id in 1..=9 {
        let bili = id <= 3;
        let offset = (id - 1) % 3;
        video::ActiveModel::from(video::Model {
            id,
            platform: if bili {
                "bilibili"
            } else if id <= 6 {
                "douyin"
            } else {
                "xiaohongshu"
            }
            .into(),
            platform_id: Some(format!("item{offset}")),
            bvid: bili.then(|| format!("item{offset}")),
            name: format!("Video {id}"),
            valid: true,
            should_download: true,
            download_status: if bili { [0, 1, 0x7fff][offset as usize] } else { 0 },
            ..Default::default()
        })
        .insert(&db)
        .await
        .unwrap();
        page::ActiveModel {
            id: Set(id),
            video_id: Set(id),
            pid: Set(1),
            name: Set("Part".into()),
            duration: Set(30),
            path: Set(Some(format!("/saved/{id}.MP4"))),
            download_status: Set(if bili && offset > 0 { 7 << 3 } else { 0 }),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        if !bili {
            db.execute_unprepared(&format!(
                "INSERT INTO {}(page_id,state,failures) VALUES({id},'{}',{})",
                if id <= 6 {
                    "douyin_download"
                } else {
                    "xiaohongshu_download"
                },
                ["waiting", "failed", "succeeded"][offset as usize],
                i32::from(offset == 1)
            ))
            .await
            .unwrap();
        }
    }
    for state in ["waiting", "failed", "succeeded"] {
        let result = list(&db, &format!("status_filter={state}&page=0&page_size=1")).await;
        assert_eq!(result["total_count"], 3);
        assert_eq!(result["videos"].as_array().unwrap().len(), 1);
        assert_eq!(result["videos"][0]["download_state"], state);
    }
    let saved = list(&db, "saved_only=true&page=0&page_size=1").await;
    assert_eq!(saved["total_count"], 4);
    assert_eq!(saved["videos"][0]["platform"], "xiaohongshu");
    assert_eq!(saved["videos"][0]["media_ready"], true);
    assert!(saved["videos"][0].get("download_status").is_none());
    assert!(saved["videos"][0]["platform_details"].get("download_status").is_none());
    let failed = list(&db, "platform=bilibili&status_filter=failed&saved_only=true").await;
    assert_eq!(failed["total_count"], 1);
    assert_eq!(failed["videos"][0]["media_ready"], true);
    assert_eq!(list(&db, "query=item0").await["total_count"], 3);
    for platform in ["bilibili", "douyin", "xiaohongshu"] {
        let value = list(
            &db,
            &format!("platform={platform}&status_filter=failed&page=0&page_size=1"),
        )
        .await;
        assert_eq!(value["total_count"], 1);
        assert_eq!(value["videos"][0]["platform"], platform);
    }
    let response = router()
        .layer(Extension(db))
        .oneshot(
            Request::builder()
                .uri("/videos?platform=unknown")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn douyin_filters_and_bili_mutations_preserve_qualified_media() {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    Migrator::up(&db, None).await.unwrap();
    video::ActiveModel::from(video::Model {
        id: 1,
        platform: "douyin".into(),
        platform_id: Some("old-video-new-collection".into()),
        name: "saved video".into(),
        download_status: 1,
        ..Default::default()
    })
    .insert(&db)
    .await
    .unwrap();
    page::ActiveModel {
        id: Set(1),
        video_id: Set(1),
        pid: Set(1),
        name: Set("saved video".into()),
        duration: Set(25),
        download_status: Set(1 << 3),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    db.execute_unprepared("INSERT INTO douyin_download(page_id,state,failures) VALUES(1,'failed',1)")
        .await
        .unwrap();
    let response = list(&db, "platform=douyin").await;
    assert_eq!(response["total_count"], 1);
    assert_eq!(response["videos"][0]["platform_id"], "old-video-new-collection");
    assert_eq!(response["videos"][0]["bvid"], Value::Null);
    assert_eq!(list(&db, "platform=bilibili").await["total_count"], 0);
    for (path, body, expected) in [
        ("/videos/1/clear-and-reset-status", json!({}), StatusCode::BAD_REQUEST),
        ("/videos/1/reset-status", json!({"force":true}), StatusCode::BAD_REQUEST),
        (
            "/videos/1/update-status",
            json!({"video_updates":[{"status_index":4,"status_value":7}]}),
            StatusCode::BAD_REQUEST,
        ),
        ("/videos/reset-status", json!({"force":true}), StatusCode::OK),
        (
            "/videos/reset-status",
            json!({"platform":"douyin"}),
            StatusCode::BAD_REQUEST,
        ),
        (
            "/videos/reset-status",
            json!({"platform":"unknown"}),
            StatusCode::BAD_REQUEST,
        ),
        ("/videos/1/reset-status", json!({}), StatusCode::BAD_REQUEST),
        (
            "/videos/update-status",
            json!({"video_updates":[{"status_index":4,"status_value":7}]}),
            StatusCode::OK,
        ),
    ] {
        let response = router()
            .layer(Extension(db.clone()))
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(path)
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected, "{path}");
        assert_eq!(
            video::Entity::find_by_id(1)
                .one(&db)
                .await
                .unwrap()
                .unwrap()
                .download_status,
            1
        );
        assert_eq!(
            page::Entity::find_by_id(1)
                .one(&db)
                .await
                .unwrap()
                .unwrap()
                .download_status,
            1 << 3
        );
    }
}

#[tokio::test]
async fn saved_only_filters_before_pagination_without_changing_existing_queries() {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    Migrator::up(&db, None).await.unwrap();
    for id in 1..=8 {
        video::ActiveModel::from(video::Model {
            id,
            name: format!("Video {id}"),
            bvid: Some(format!("BV{id}")),
            platform_id: Some(format!("BV{id}")),
            upper_name: format!("Author{id}"),
            // Saved files remain watchable even if the source is now invalid or skipped.
            valid: false,
            should_download: false,
            ..Default::default()
        })
        .insert(&db)
        .await
        .unwrap();
    }
    for (id, video_id, path, status) in [
        (1, 1, Some("partial.mp4"), 0),
        (2, 1, Some("saved.mp4"), 1 | (7 << 3) | (1 << 12)),
        (3, 2, Some("saved.MP4"), 7 << 3),
        (4, 4, None, 7 << 3),
        (5, 5, Some("saved.mkv"), 7 << 3),
        (6, 6, Some("unfinished.mp4"), 0),
        (7, 7, Some(""), 7 << 3),
        (8, 8, Some("first.mp4"), 7 << 3),
        (9, 8, Some("second.mp4"), 7 << 3),
    ] {
        page::ActiveModel {
            id: Set(id),
            video_id: Set(video_id),
            cid: Set(Some(id.into())),
            pid: Set(id),
            name: Set(format!("Part {id}")),
            duration: Set(60),
            path: Set(path.map(str::to_owned)),
            download_status: Set(status),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
    }

    let first = list(&db, "saved_only=true&page=0&page_size=2").await;
    assert_eq!(first["total_count"], 3);
    assert_eq!(first["videos"][0]["id"], 8);
    assert_eq!(first["videos"][1]["id"], 2);
    let second = list(&db, "saved_only=true&page=1&page_size=2").await;
    assert_eq!(second["total_count"], 3);
    assert_eq!(second["videos"].as_array().unwrap().len(), 1);
    assert_eq!(second["videos"][0]["id"], 1);
    assert_eq!(
        list(&db, "saved_only=true&page=2&page_size=2").await["videos"],
        json!([])
    );
    for query in ["Video%201", "BV1"] {
        let found = list(&db, &format!("saved_only=true&query={query}")).await;
        assert_eq!(found["total_count"], 1);
        assert_eq!(found["videos"][0]["id"], 1);
    }
    assert_eq!(list(&db, "saved_only=true&query=BV3").await["total_count"], 0);
    let existing = list(&db, "page=0&page_size=20").await;
    assert_eq!(existing["total_count"], 8);
    assert_eq!(existing, list(&db, "saved_only=false&page=0&page_size=20").await);
    assert_eq!(list(&db, "query=BV3").await["total_count"], 1);
    assert!(existing.get("watch_summaries").is_none());
    assert_eq!(list(&db, "saved_only=true&query=Author1").await["total_count"], 1);
    assert_eq!(list(&db, "saved_only=true&sort=title").await["videos"][0]["id"], 1);
    assert_eq!(first["watch_summaries"][0]["page_count"], 2);
    assert_eq!(first["watch_summaries"][0]["duration"], 120);
    assert_eq!(
        list(&db, "saved_only=true&watch_state=unwatched").await["total_count"],
        3
    );
    for (id, position, completed) in [(1, 60000, true), (2, 60000, true), (3, 12000, false)] {
        watch_progress::ActiveModel {
            page_id: Set(id),
            position_ms: Set(position),
            completed: Set(completed),
            revision: Set(1),
            updated_at: Set(chrono::Utc::now().naive_utc()),
        }
        .insert(&db)
        .await
        .unwrap();
    }
    for (state, id) in [("unwatched", 8), ("in_progress", 2), ("watched", 1)] {
        let found = list(&db, &format!("saved_only=true&watch_state={state}&page=0&page_size=1")).await;
        assert_eq!(found["total_count"], 1);
        assert_eq!(found["videos"][0]["id"], id);
        assert_eq!(found["watch_summaries"][0]["watch_state"], state);
    }
    assert_eq!(
        list(&db, "saved_only=true&watch_state=in_progress").await["watch_summaries"][0]["resume_position_ms"],
        12000
    );
    watch_progress::ActiveModel {
        page_id: Set(8),
        position_ms: Set(60000),
        completed: Set(true),
        revision: Set(1),
        updated_at: Set(chrono::Utc::now().naive_utc()),
    }
    .insert(&db)
    .await
    .unwrap();
    let partial = list(&db, "saved_only=true&watch_state=in_progress&page=0&page_size=1").await;
    assert_eq!(partial["total_count"], 2);
    assert_eq!(partial["watch_summaries"][0]["resume_page_id"], 9);
    watch_progress::ActiveModel {
        page_id: Set(9),
        position_ms: Set(60000),
        completed: Set(true),
        revision: Set(1),
        updated_at: Set(chrono::Utc::now().naive_utc()),
    }
    .insert(&db)
    .await
    .unwrap();
    assert_eq!(list(&db, "saved_only=true&watch_state=watched").await["total_count"], 2);
}
