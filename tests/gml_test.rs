//! Integration tests for the GML reader.

use rs_shapefile::gml_reader::GmlReader;
use rs_shapefile::{AttributeValue, BoundingBox, Geometry};

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

fn wrap_gml(features: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<ksj:Dataset
    xmlns:ksj="http://nlftp.mlit.go.jp/ksj/schemas/ksj-app"
    xmlns:gml="http://www.opengis.net/gml"
    xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
{features}
</ksj:Dataset>"#
    )
}

fn make_road_feature(
    id: &str,
    coords: &str,
    rdctg: i32,
    rnk_width: i32,
    route_name: &str,
) -> String {
    format!(
        r#"
  <gml:featureMember>
    <ksj:Road gml:id="{id}">
      <ksj:location>
        <gml:LineString srsName="http://www.opengis.net/def/crs/EPSG/0/6668">
          <gml:posList>{coords}</gml:posList>
        </gml:LineString>
      </ksj:location>
      <ksj:rdCtg>{rdctg}</ksj:rdCtg>
      <ksj:rnkWidth>{rnk_width}</ksj:rnkWidth>
      <ksj:routeName>{route_name}</ksj:routeName>
    </ksj:Road>
  </gml:featureMember>"#
    )
}

fn sample_xml() -> String {
    let f1 = make_road_feature("r1", "35.6895 139.6917 35.6900 139.6920", 1, 5, "Route 1");
    let f2 = make_road_feature("r2", "35.7000 139.7000 35.7010 139.7010", 2, 3, "Route 246");
    let f3 = make_road_feature(
        "r3",
        "35.6800 139.6800 35.6810 139.6810",
        1,
        7,
        "Meiji-dori",
    );
    wrap_gml(&format!("{f1}{f2}{f3}"))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn test_from_str() {
    let xml = sample_xml();
    let reader = GmlReader::from_str(&xml).unwrap();
    assert!(reader.len() > 0);
}

#[test]
fn test_len_positive() {
    let xml = sample_xml();
    let reader = GmlReader::from_str(&xml).unwrap();
    assert_eq!(reader.len(), 3);
}

#[test]
fn test_bbox_valid() {
    let xml = sample_xml();
    let reader = GmlReader::from_str(&xml).unwrap();
    let bb = reader.bbox();
    // longitude should be around 139
    assert!(bb.x_min > 139.0 && bb.x_min < 140.0);
    assert!(bb.x_max > 139.0 && bb.x_max < 140.0);
    // latitude should be around 35
    assert!(bb.y_min > 35.0 && bb.y_min < 36.0);
    assert!(bb.y_max > 35.0 && bb.y_max < 36.0);
}

#[test]
fn test_srs_name() {
    let xml = sample_xml();
    let reader = GmlReader::from_str(&xml).unwrap();
    let srs = reader.srs_name().unwrap();
    assert!(srs.contains("6668"));
}

#[test]
fn test_records_limit() {
    let xml = sample_xml();
    let reader = GmlReader::from_str(&xml).unwrap();
    let recs = reader.records(Some(1));
    assert_eq!(recs.len(), 1);
}

#[test]
fn test_iter_records_count() {
    let xml = sample_xml();
    let reader = GmlReader::from_str(&xml).unwrap();
    assert_eq!(reader.iter_records().count(), reader.len());
}

#[test]
fn test_get_first_record() {
    let xml = sample_xml();
    let reader = GmlReader::from_str(&xml).unwrap();
    assert!(reader.get(0).is_some());
}

#[test]
fn test_get_out_of_range() {
    let xml = sample_xml();
    let reader = GmlReader::from_str(&xml).unwrap();
    assert!(reader.get(9999).is_none());
}

#[test]
fn test_filter_by_rdctg() {
    let xml = sample_xml();
    let reader = GmlReader::from_str(&xml).unwrap();
    let filtered = reader.filter_by_attribute("rdCtg", &AttributeValue::Numeric(1.0));
    assert_eq!(filtered.len(), 2);
}

#[test]
fn test_filter_by_rdctg_list() {
    let xml = sample_xml();
    let reader = GmlReader::from_str(&xml).unwrap();
    let filtered = reader.filter_by_attribute_in(
        "rdCtg",
        &[AttributeValue::Numeric(1.0), AttributeValue::Numeric(2.0)],
    );
    assert_eq!(filtered.len(), 3);
}

#[test]
fn test_filter_by_bbox() {
    let xml = sample_xml();
    let reader = GmlReader::from_str(&xml).unwrap();
    let bbox = BoundingBox {
        x_min: 139.69,
        y_min: 35.68,
        x_max: 139.70,
        y_max: 35.70,
    };
    let filtered = reader.filter_by_bbox(&bbox);
    assert!(!filtered.is_empty());
}

#[test]
fn test_filter_starts_with_route() {
    let xml = sample_xml();
    let reader = GmlReader::from_str(&xml).unwrap();
    let filtered = reader.filter_by_attribute_starts_with("routeName", "Route");
    assert_eq!(filtered.len(), 2);
}

#[test]
fn test_describe_rnk_width() {
    let xml = sample_xml();
    let reader = GmlReader::from_str(&xml).unwrap();
    let stats = reader.describe("rnkWidth").unwrap();
    assert_eq!(stats.count, 3);
    assert!((stats.min - 3.0).abs() < 1e-10);
    assert!((stats.max - 7.0).abs() < 1e-10);
    assert!((stats.mean - 5.0).abs() < 1e-10);
    assert!((stats.median - 5.0).abs() < 1e-10);
}

#[test]
fn test_describe_string_field_error() {
    let xml = sample_xml();
    let reader = GmlReader::from_str(&xml).unwrap();
    let result = reader.describe("routeName");
    assert!(result.is_err());
}

#[test]
fn test_coord_order_lon_lat() {
    let xml = sample_xml();
    let reader = GmlReader::from_str(&xml).unwrap();
    let rec = reader.get(0).unwrap();
    if let Geometry::Polyline(pl) = &rec.geometry {
        let p = &pl.parts[0][0];
        // x should be longitude (~139), y should be latitude (~35)
        assert!(
            p.x > 139.0 && p.x < 140.0,
            "x should be longitude, got {}",
            p.x
        );
        assert!(
            p.y > 35.0 && p.y < 36.0,
            "y should be latitude, got {}",
            p.y
        );
    } else {
        panic!("expected Polyline geometry");
    }
}

#[test]
fn test_point_geometry() {
    let xml = wrap_gml(
        r#"
  <gml:featureMember>
    <ksj:Station gml:id="s1">
      <ksj:location>
        <gml:Point srsName="http://www.opengis.net/def/crs/EPSG/0/6668">
          <gml:pos>35.6895 139.6917</gml:pos>
        </gml:Point>
      </ksj:location>
      <ksj:name>Tokyo</ksj:name>
    </ksj:Station>
  </gml:featureMember>"#,
    );
    let reader = GmlReader::from_str(&xml).unwrap();
    assert_eq!(reader.len(), 1);
    let rec = reader.get(0).unwrap();
    assert!(matches!(rec.geometry, Geometry::Point(_)));
    if let Geometry::Point(p) = &rec.geometry {
        assert!((p.x - 139.6917).abs() < 1e-4);
        assert!((p.y - 35.6895).abs() < 1e-4);
    }
}

#[test]
fn test_polygon_geometry() {
    let xml = wrap_gml(
        r#"
  <gml:featureMember>
    <ksj:Area gml:id="a1">
      <ksj:location>
        <gml:Polygon srsName="http://www.opengis.net/def/crs/EPSG/0/6668">
          <gml:exterior>
            <gml:LinearRing>
              <gml:posList>35.0 139.0 35.1 139.0 35.1 139.1 35.0 139.1 35.0 139.0</gml:posList>
            </gml:LinearRing>
          </gml:exterior>
        </gml:Polygon>
      </ksj:location>
      <ksj:areaName>Shinjuku</ksj:areaName>
    </ksj:Area>
  </gml:featureMember>"#,
    );
    let reader = GmlReader::from_str(&xml).unwrap();
    assert_eq!(reader.len(), 1);
    let rec = reader.get(0).unwrap();
    assert!(matches!(rec.geometry, Geometry::Polygon(_)));
    if let Geometry::Polygon(pg) = &rec.geometry {
        assert_eq!(pg.rings.len(), 1);
        assert_eq!(pg.exterior().points.len(), 5);
    }
}
