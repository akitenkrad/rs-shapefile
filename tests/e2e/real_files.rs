mod n13_large_file;

use std::path::PathBuf;

#[allow(unused_imports)]
use rs_shapefile::*;

#[allow(dead_code)]
fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/shp")
        .join(name)
}

macro_rules! fixture_test {
    ($name:ident, $fixture:expr, $body:block) => {
        #[test]
        fn $name() {
            let path = fixture_path($fixture);
            if !path.exists() {
                eprintln!("skip: fixture not found: {}", path.display());
                return;
            }
            $body
        }
    };
}

fixture_test!(test_read_point_shapefile, "point.shp", {
    let mut sf = ShapefileReader::open(fixture_path("point.shp")).unwrap();
    assert_eq!(sf.shape_type(), ShapeType::Point);
    assert!(!sf.records(None).unwrap().is_empty());
});

fixture_test!(test_read_polyline_shapefile, "polyline.shp", {
    let mut sf = ShapefileReader::open(fixture_path("polyline.shp")).unwrap();
    assert_eq!(sf.shape_type(), ShapeType::Polyline);
    let records = sf.records(None).unwrap();
    assert!(!records.is_empty());
    assert!(records.iter().all(|r| !r.geometry.is_null()));
});

fixture_test!(test_read_polygon_shapefile, "polygon.shp", {
    let sf = ShapefileReader::open(fixture_path("polygon.shp")).unwrap();
    assert_eq!(sf.shape_type(), ShapeType::Polygon);
});
