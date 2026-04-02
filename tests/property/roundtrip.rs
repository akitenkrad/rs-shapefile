use proptest::prelude::*;
use rs_shapefile::{BoundingBox, HasGeometry, Point, Polyline};

proptest! {
    #[test]
    fn point_bbox_contains_itself(x in -1e15f64..1e15, y in -1e15f64..1e15) {
        let p = Point { x, y };
        prop_assert!(p.bbox().contains(x, y));
    }

    #[test]
    fn point_distance_symmetry(
        x1 in -180f64..180f64, y1 in -90f64..90f64,
        x2 in -180f64..180f64, y2 in -90f64..90f64,
    ) {
        let a = Point { x: x1, y: y1 };
        let b = Point { x: x2, y: y2 };
        prop_assert!((a.distance_to(&b) - b.distance_to(&a)).abs() < 1e-9);
    }

    #[test]
    fn polyline_2pt_length_equals_distance(
        x1 in -180f64..180f64, y1 in -90f64..90f64,
        x2 in -180f64..180f64, y2 in -90f64..90f64,
    ) {
        let a = Point { x: x1, y: y1 };
        let b = Point { x: x2, y: y2 };
        let line = Polyline { parts: vec![vec![a, b]] };
        prop_assert!((line.length() - a.distance_to(&b)).abs() < 1e-9);
    }

    #[test]
    fn polyline_num_points_correct(
        pts in proptest::collection::vec(
            (-180f64..180f64, -90f64..90f64),
            2..=20usize,
        )
    ) {
        let points: Vec<Point> = pts.iter().map(|(x, y)| Point { x: *x, y: *y }).collect();
        let n = points.len();
        let line = Polyline { parts: vec![points] };
        prop_assert_eq!(line.num_points(), n);
        prop_assert_eq!(line.num_parts(), 1);
    }

    #[test]
    fn polyline_length_nonnegative(
        x1 in -180f64..180f64, y1 in -90f64..90f64,
        x2 in -180f64..180f64, y2 in -90f64..90f64,
    ) {
        let line = Polyline { parts: vec![vec![Point { x: x1, y: y1 }, Point { x: x2, y: y2 }]] };
        prop_assert!(line.length() >= 0.0);
    }

    #[test]
    fn bbox_intersects_self(
        x_min in -180f64..0f64, y_min in -90f64..0f64,
        x_max in 0f64..180f64,  y_max in 0f64..90f64,
    ) {
        let bb = BoundingBox { x_min, y_min, x_max, y_max };
        prop_assert!(bb.intersects(&bb));
    }
}
