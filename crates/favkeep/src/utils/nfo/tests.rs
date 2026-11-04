use super::*;

#[tokio::test]
async fn test_generate_nfo() {
    let video = video::Model {
        intro: "intro".to_string(),
        name: "name".to_string(),
        upper_id: Some(1),
        upper_name: "upper_name".to_string(),
        upper_face: "https://i1.hdslb.com/bfs/face/72e8f33cadc72e022fc34624cc69e1b12ebb72c0.jpg".to_string(),
        favtime: chrono::NaiveDateTime::new(
            chrono::NaiveDate::from_ymd_opt(2022, 2, 2).unwrap(),
            chrono::NaiveTime::from_hms_opt(2, 2, 2).unwrap(),
        ),
        pubtime: chrono::NaiveDateTime::new(
            chrono::NaiveDate::from_ymd_opt(2033, 3, 3).unwrap(),
            chrono::NaiveTime::from_hms_opt(3, 3, 3).unwrap(),
        ),
        bvid: Some("BV1nWcSeeEkV".to_string()),
        tags: Some(vec!["tag1".to_owned(), "tag2".to_owned()].into()),
        ..Default::default()
    };
    assert_eq!(
        NFO::Movie((&video).to_nfo(NFOTimeType::FavTime))
            .generate_nfo()
            .await
            .unwrap(),
        r#"<?xml version="1.0" encoding="utf-8" standalone="yes"?>
<movie>
    <plot><![CDATA[原始视频：<a href="https://www.bilibili.com/video/BV1nWcSeeEkV/">BV1nWcSeeEkV</a><br/><br/>intro]]></plot>
    <outline/>
    <title>name</title>
    <actor>
        <name>1</name>
        <role>upper_name</role>
        <thumb>https://i1.hdslb.com/bfs/face/72e8f33cadc72e022fc34624cc69e1b12ebb72c0.jpg</thumb>
    </actor>
    <year>2022</year>
    <genre>tag1</genre>
    <genre>tag2</genre>
    <uniqueid type="bilibili">BV1nWcSeeEkV</uniqueid>
    <premiered>2022-02-02</premiered>
</movie>"#,
    );
    assert_eq!(
        NFO::TVShow((&video).to_nfo(NFOTimeType::FavTime))
            .generate_nfo()
            .await
            .unwrap(),
        r#"<?xml version="1.0" encoding="utf-8" standalone="yes"?>
<tvshow>
    <plot><![CDATA[原始视频：<a href="https://www.bilibili.com/video/BV1nWcSeeEkV/">BV1nWcSeeEkV</a><br/><br/>intro]]></plot>
    <outline/>
    <title>name</title>
    <actor>
        <name>1</name>
        <role>upper_name</role>
        <thumb>https://i1.hdslb.com/bfs/face/72e8f33cadc72e022fc34624cc69e1b12ebb72c0.jpg</thumb>
    </actor>
    <year>2022</year>
    <genre>tag1</genre>
    <genre>tag2</genre>
    <uniqueid type="bilibili">BV1nWcSeeEkV</uniqueid>
    <premiered>2022-02-02</premiered>
</tvshow>"#,
    );
    assert_eq!(
        NFO::Upper((&video, &video.uppers().next().unwrap()).to_nfo(NFOTimeType::FavTime))
            .generate_nfo()
            .await
            .unwrap(),
        r#"<?xml version="1.0" encoding="utf-8" standalone="yes"?>
<person>
    <plot/>
    <outline/>
    <lockdata>false</lockdata>
    <dateadded>2033-03-03 03:03:03</dateadded>
    <title>1</title>
    <sorttitle>1</sorttitle>
</person>"#,
    );
    let page = page::Model {
        name: "name".to_string(),
        pid: 3,
        ..Default::default()
    };
    assert_eq!(
        NFO::Episode((&page).to_nfo(NFOTimeType::FavTime))
            .generate_nfo()
            .await
            .unwrap(),
        r#"<?xml version="1.0" encoding="utf-8" standalone="yes"?>
<episodedetails>
    <plot/>
    <outline/>
    <title>name</title>
    <season>1</season>
    <episode>3</episode>
</episodedetails>"#,
    );
}
