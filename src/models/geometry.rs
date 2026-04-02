use crate::models::bbox::BoundingBox;
use crate::parsers::shp_parser::ShapeType;

/// Trait for types that have a spatial geometry with a shape type and bounding box.
pub trait HasGeometry {
    /// Returns the ESRI shape type of this geometry.
    fn shape_type(&self) -> ShapeType;
    /// Returns the axis-aligned bounding box of this geometry.
    fn bbox(&self) -> BoundingBox;
}

/// A 2D point with x (longitude) and y (latitude) coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    /// X coordinate (typically longitude).
    pub x: f64,
    /// Y coordinate (typically latitude).
    pub y: f64,
}

impl Point {
    /// Euclidean distance. For geographic CRS (degree units), this is an approximation.
    pub fn distance_to(&self, other: &Point) -> f64 {
        ((self.x - other.x).powi(2) + (self.y - other.y).powi(2)).sqrt()
    }
}

impl HasGeometry for Point {
    fn shape_type(&self) -> ShapeType {
        ShapeType::Point
    }
    fn bbox(&self) -> BoundingBox {
        BoundingBox {
            x_min: self.x,
            y_min: self.y,
            x_max: self.x,
            y_max: self.y,
        }
    }
}

/// An ordered set of vertices forming one or more line segments (parts).
#[derive(Debug, Clone, PartialEq)]
pub struct Polyline {
    /// Each inner `Vec<Point>` is one part of the polyline.
    pub parts: Vec<Vec<Point>>,
}

impl Polyline {
    /// Returns the number of parts in this polyline.
    pub fn num_parts(&self) -> usize {
        self.parts.len()
    }

    /// Returns the total number of vertices across all parts.
    pub fn num_points(&self) -> usize {
        self.parts.iter().map(|p| p.len()).sum()
    }

    /// Total length of all parts. For geographic CRS, returns degree-unit approximation.
    pub fn length(&self) -> f64 {
        self.parts
            .iter()
            .map(|part| {
                part.windows(2)
                    .map(|w| w[0].distance_to(&w[1]))
                    .sum::<f64>()
            })
            .sum()
    }
}

impl HasGeometry for Polyline {
    fn shape_type(&self) -> ShapeType {
        ShapeType::Polyline
    }
    fn bbox(&self) -> BoundingBox {
        let pts: Vec<&Point> = self.parts.iter().flatten().collect();
        BoundingBox {
            x_min: pts.iter().map(|p| p.x).fold(f64::INFINITY, f64::min),
            y_min: pts.iter().map(|p| p.y).fold(f64::INFINITY, f64::min),
            x_max: pts.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max),
            y_max: pts.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max),
        }
    }
}

/// A closed ring of vertices forming part of a polygon (exterior or hole).
#[derive(Debug, Clone, PartialEq)]
pub struct Ring {
    /// Vertices of the ring; first and last point should be identical.
    pub points: Vec<Point>,
}

impl Ring {
    /// Shoelace formula sign check (negative -> clockwise = ESRI exterior ring)
    pub fn is_clockwise(&self) -> bool {
        let n = self.points.len();
        let area: f64 = (0..n)
            .map(|i| {
                let j = (i + 1) % n;
                self.points[i].x * self.points[j].y - self.points[j].x * self.points[i].y
            })
            .sum();
        area < 0.0
    }

    /// Returns the unsigned area of this ring using the Shoelace formula.
    pub fn area(&self) -> f64 {
        let n = self.points.len();
        (0..n)
            .map(|i| {
                let j = (i + 1) % n;
                self.points[i].x * self.points[j].y - self.points[j].x * self.points[i].y
            })
            .sum::<f64>()
            .abs()
            / 2.0
    }
}

/// A polygon consisting of one exterior ring and zero or more interior holes.
#[derive(Debug, Clone, PartialEq)]
pub struct Polygon {
    /// `rings[0]` is the exterior ring; `rings[1..]` are holes.
    pub rings: Vec<Ring>,
}

impl Polygon {
    /// Returns a reference to the exterior ring.
    pub fn exterior(&self) -> &Ring {
        &self.rings[0]
    }

    /// Returns a slice of interior hole rings (empty if none).
    pub fn holes(&self) -> &[Ring] {
        if self.rings.len() > 1 {
            &self.rings[1..]
        } else {
            &[]
        }
    }

    /// Area (exterior minus holes). For geographic CRS, returns degree^2 approximation.
    pub fn area(&self) -> f64 {
        self.exterior().area() - self.holes().iter().map(|h| h.area()).sum::<f64>()
    }

    /// Perimeter (all rings). For geographic CRS, returns degree-unit approximation.
    pub fn perimeter(&self) -> f64 {
        self.rings
            .iter()
            .map(|r| {
                r.points
                    .windows(2)
                    .map(|w| w[0].distance_to(&w[1]))
                    .sum::<f64>()
            })
            .sum()
    }
}

impl HasGeometry for Polygon {
    fn shape_type(&self) -> ShapeType {
        ShapeType::Polygon
    }
    fn bbox(&self) -> BoundingBox {
        let pts: Vec<&Point> = self.rings.iter().flat_map(|r| r.points.iter()).collect();
        BoundingBox {
            x_min: pts.iter().map(|p| p.x).fold(f64::INFINITY, f64::min),
            y_min: pts.iter().map(|p| p.y).fold(f64::INFINITY, f64::min),
            x_max: pts.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max),
            y_max: pts.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max),
        }
    }
}

/// A 3D point with x, y, z coordinates and an optional measure value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointZ {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub m: Option<f64>,
}

/// A polyline with Z (elevation) coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct PolylineZ {
    pub parts: Vec<Vec<PointZ>>,
}

/// A polygon with Z (elevation) coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct PolygonZ {
    pub rings: Vec<Vec<PointZ>>,
}

/// A 2D point with an associated measure (M) value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointM {
    pub x: f64,
    pub y: f64,
    pub m: f64,
}

/// A polyline with measure (M) values.
#[derive(Debug, Clone, PartialEq)]
pub struct PolylineM {
    pub parts: Vec<Vec<PointM>>,
}

/// A polygon with measure (M) values.
#[derive(Debug, Clone, PartialEq)]
pub struct PolygonM {
    pub rings: Vec<Vec<PointM>>,
}

/// A set of unconnected points.
#[derive(Debug, Clone, PartialEq)]
pub struct MultiPoint {
    /// The individual points in this collection.
    pub points: Vec<Point>,
}

impl HasGeometry for MultiPoint {
    fn shape_type(&self) -> ShapeType {
        ShapeType::MultiPoint
    }
    fn bbox(&self) -> BoundingBox {
        BoundingBox {
            x_min: self
                .points
                .iter()
                .map(|p| p.x)
                .fold(f64::INFINITY, f64::min),
            y_min: self
                .points
                .iter()
                .map(|p| p.y)
                .fold(f64::INFINITY, f64::min),
            x_max: self
                .points
                .iter()
                .map(|p| p.x)
                .fold(f64::NEG_INFINITY, f64::max),
            y_max: self
                .points
                .iter()
                .map(|p| p.y)
                .fold(f64::NEG_INFINITY, f64::max),
        }
    }
}

impl HasGeometry for PointZ {
    fn shape_type(&self) -> ShapeType {
        ShapeType::PointZ
    }
    fn bbox(&self) -> BoundingBox {
        BoundingBox {
            x_min: self.x,
            y_min: self.y,
            x_max: self.x,
            y_max: self.y,
        }
    }
}

impl HasGeometry for PolylineZ {
    fn shape_type(&self) -> ShapeType {
        ShapeType::PolylineZ
    }
    fn bbox(&self) -> BoundingBox {
        let pts: Vec<&PointZ> = self.parts.iter().flatten().collect();
        BoundingBox {
            x_min: pts.iter().map(|p| p.x).fold(f64::INFINITY, f64::min),
            y_min: pts.iter().map(|p| p.y).fold(f64::INFINITY, f64::min),
            x_max: pts.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max),
            y_max: pts.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max),
        }
    }
}

impl HasGeometry for PolygonZ {
    fn shape_type(&self) -> ShapeType {
        ShapeType::PolygonZ
    }
    fn bbox(&self) -> BoundingBox {
        let pts: Vec<&PointZ> = self.rings.iter().flatten().collect();
        BoundingBox {
            x_min: pts.iter().map(|p| p.x).fold(f64::INFINITY, f64::min),
            y_min: pts.iter().map(|p| p.y).fold(f64::INFINITY, f64::min),
            x_max: pts.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max),
            y_max: pts.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max),
        }
    }
}

impl HasGeometry for PointM {
    fn shape_type(&self) -> ShapeType {
        ShapeType::PointM
    }
    fn bbox(&self) -> BoundingBox {
        BoundingBox {
            x_min: self.x,
            y_min: self.y,
            x_max: self.x,
            y_max: self.y,
        }
    }
}

impl HasGeometry for PolylineM {
    fn shape_type(&self) -> ShapeType {
        ShapeType::PolylineM
    }
    fn bbox(&self) -> BoundingBox {
        let pts: Vec<&PointM> = self.parts.iter().flatten().collect();
        BoundingBox {
            x_min: pts.iter().map(|p| p.x).fold(f64::INFINITY, f64::min),
            y_min: pts.iter().map(|p| p.y).fold(f64::INFINITY, f64::min),
            x_max: pts.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max),
            y_max: pts.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max),
        }
    }
}

impl HasGeometry for PolygonM {
    fn shape_type(&self) -> ShapeType {
        ShapeType::PolygonM
    }
    fn bbox(&self) -> BoundingBox {
        let pts: Vec<&PointM> = self.rings.iter().flatten().collect();
        BoundingBox {
            x_min: pts.iter().map(|p| p.x).fold(f64::INFINITY, f64::min),
            y_min: pts.iter().map(|p| p.y).fold(f64::INFINITY, f64::min),
            x_max: pts.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max),
            y_max: pts.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max),
        }
    }
}

/// Algebraic data type representing all supported ESRI geometry variants.
#[derive(Debug, Clone, PartialEq)]
pub enum Geometry {
    /// Null shape (no geometry).
    Null,
    /// A 2D point.
    Point(Point),
    /// A 2D polyline with one or more parts.
    Polyline(Polyline),
    /// A 2D polygon with exterior and optional hole rings.
    Polygon(Polygon),
    /// A collection of 2D points.
    MultiPoint(MultiPoint),
    /// A 3D point with Z coordinate.
    PointZ(PointZ),
    /// A 3D polyline with Z coordinates.
    PolylineZ(PolylineZ),
    /// A 3D polygon with Z coordinates.
    PolygonZ(PolygonZ),
    /// A 2D point with a measure value.
    PointM(PointM),
    /// A 2D polyline with measure values.
    PolylineM(PolylineM),
    /// A 2D polygon with measure values.
    PolygonM(PolygonM),
}

impl Geometry {
    /// Returns the ESRI shape type of the contained geometry.
    pub fn shape_type(&self) -> ShapeType {
        match self {
            Geometry::Null => ShapeType::Null,
            Geometry::Point(_) => ShapeType::Point,
            Geometry::Polyline(_) => ShapeType::Polyline,
            Geometry::Polygon(_) => ShapeType::Polygon,
            Geometry::MultiPoint(_) => ShapeType::MultiPoint,
            Geometry::PointZ(_) => ShapeType::PointZ,
            Geometry::PolylineZ(_) => ShapeType::PolylineZ,
            Geometry::PolygonZ(_) => ShapeType::PolygonZ,
            Geometry::PointM(_) => ShapeType::PointM,
            Geometry::PolylineM(_) => ShapeType::PolylineM,
            Geometry::PolygonM(_) => ShapeType::PolygonM,
        }
    }

    /// Returns the bounding box, or `None` for null geometries.
    pub fn bbox(&self) -> Option<BoundingBox> {
        match self {
            Geometry::Null => None,
            Geometry::Point(p) => Some(p.bbox()),
            Geometry::Polyline(p) => Some(p.bbox()),
            Geometry::Polygon(p) => Some(p.bbox()),
            Geometry::MultiPoint(p) => Some(p.bbox()),
            Geometry::PointZ(p) => Some(p.bbox()),
            Geometry::PolylineZ(p) => Some(p.bbox()),
            Geometry::PolygonZ(p) => Some(p.bbox()),
            Geometry::PointM(p) => Some(p.bbox()),
            Geometry::PolylineM(p) => Some(p.bbox()),
            Geometry::PolygonM(p) => Some(p.bbox()),
        }
    }

    /// Returns a reference to the inner `Point`, if this is a Point geometry.
    pub fn as_point(&self) -> Option<&Point> {
        match self {
            Geometry::Point(p) => Some(p),
            _ => None,
        }
    }

    /// Returns a reference to the inner `Polyline`, if this is a Polyline geometry.
    pub fn as_polyline(&self) -> Option<&Polyline> {
        match self {
            Geometry::Polyline(p) => Some(p),
            _ => None,
        }
    }

    /// Returns a reference to the inner `Polygon`, if this is a Polygon geometry.
    pub fn as_polygon(&self) -> Option<&Polygon> {
        match self {
            Geometry::Polygon(p) => Some(p),
            _ => None,
        }
    }

    /// Returns `true` if this is a null geometry.
    pub fn is_null(&self) -> bool {
        matches!(self, Geometry::Null)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_point_bbox() {
        let p = Point { x: 1.0, y: 2.0 };
        let bb = p.bbox();
        assert_eq!(bb.x_min, 1.0);
        assert_eq!(bb.x_max, 1.0);
        assert_eq!(bb.y_min, 2.0);
        assert_eq!(bb.y_max, 2.0);
    }

    #[test]
    fn test_point_distance_to_self() {
        let p = Point { x: 1.0, y: 2.0 };
        assert!((p.distance_to(&p) - 0.0).abs() < 1e-15);
    }

    #[test]
    fn test_point_distance_symmetry() {
        let a = Point { x: 0.0, y: 0.0 };
        let b = Point { x: 3.0, y: 4.0 };
        assert!((a.distance_to(&b) - b.distance_to(&a)).abs() < 1e-15);
    }

    #[test]
    fn test_polyline_num_parts_single() {
        let line = Polyline {
            parts: vec![vec![Point { x: 0.0, y: 0.0 }, Point { x: 1.0, y: 1.0 }]],
        };
        assert_eq!(line.num_parts(), 1);
    }

    #[test]
    fn test_polyline_num_parts_multi() {
        let line = Polyline {
            parts: vec![
                vec![Point { x: 0.0, y: 0.0 }, Point { x: 1.0, y: 1.0 }],
                vec![Point { x: 2.0, y: 2.0 }, Point { x: 3.0, y: 3.0 }],
            ],
        };
        assert_eq!(line.num_parts(), 2);
    }

    #[test]
    fn test_polyline_num_points() {
        let line = Polyline {
            parts: vec![
                vec![Point { x: 0.0, y: 0.0 }, Point { x: 1.0, y: 1.0 }],
                vec![
                    Point { x: 2.0, y: 2.0 },
                    Point { x: 3.0, y: 3.0 },
                    Point { x: 4.0, y: 4.0 },
                ],
            ],
        };
        assert_eq!(line.num_points(), 5);
    }

    #[test]
    fn test_polyline_length_2pt() {
        let a = Point { x: 0.0, y: 0.0 };
        let b = Point { x: 3.0, y: 4.0 };
        let line = Polyline {
            parts: vec![vec![a, b]],
        };
        assert!((line.length() - a.distance_to(&b)).abs() < 1e-15);
    }

    #[test]
    fn test_polyline_length_multipart() {
        let line = Polyline {
            parts: vec![
                vec![Point { x: 0.0, y: 0.0 }, Point { x: 3.0, y: 4.0 }],
                vec![Point { x: 0.0, y: 0.0 }, Point { x: 0.0, y: 1.0 }],
            ],
        };
        assert!((line.length() - 6.0).abs() < 1e-15);
    }

    #[test]
    fn test_polyline_length_nonnegative() {
        let line = Polyline {
            parts: vec![vec![Point { x: -1.0, y: -1.0 }, Point { x: 1.0, y: 1.0 }]],
        };
        assert!(line.length() >= 0.0);
    }

    #[test]
    fn test_polyline_bbox_covers_all_points() {
        let line = Polyline {
            parts: vec![vec![
                Point { x: 1.0, y: 2.0 },
                Point { x: 3.0, y: 4.0 },
                Point { x: 0.5, y: 0.5 },
            ]],
        };
        let bb = line.bbox();
        for pt in line.parts.iter().flatten() {
            assert!(bb.contains(pt.x, pt.y));
        }
    }

    #[test]
    fn test_ring_is_clockwise_cw() {
        // Clockwise: (0,0) -> (0,1) -> (1,1) -> (1,0) -> (0,0)
        let ring = Ring {
            points: vec![
                Point { x: 0.0, y: 0.0 },
                Point { x: 0.0, y: 1.0 },
                Point { x: 1.0, y: 1.0 },
                Point { x: 1.0, y: 0.0 },
                Point { x: 0.0, y: 0.0 },
            ],
        };
        assert!(ring.is_clockwise());
    }

    #[test]
    fn test_ring_is_clockwise_ccw() {
        // Counter-clockwise: (0,0) -> (1,0) -> (1,1) -> (0,1) -> (0,0)
        let ring = Ring {
            points: vec![
                Point { x: 0.0, y: 0.0 },
                Point { x: 1.0, y: 0.0 },
                Point { x: 1.0, y: 1.0 },
                Point { x: 0.0, y: 1.0 },
                Point { x: 0.0, y: 0.0 },
            ],
        };
        assert!(!ring.is_clockwise());
    }

    #[test]
    fn test_ring_area() {
        // Unit square: area = 1.0
        let ring = Ring {
            points: vec![
                Point { x: 0.0, y: 0.0 },
                Point { x: 1.0, y: 0.0 },
                Point { x: 1.0, y: 1.0 },
                Point { x: 0.0, y: 1.0 },
                Point { x: 0.0, y: 0.0 },
            ],
        };
        assert!((ring.area() - 1.0).abs() < 1e-15);
    }

    #[test]
    fn test_polygon_exterior() {
        let exterior = Ring {
            points: vec![
                Point { x: 0.0, y: 0.0 },
                Point { x: 4.0, y: 0.0 },
                Point { x: 4.0, y: 4.0 },
                Point { x: 0.0, y: 4.0 },
                Point { x: 0.0, y: 0.0 },
            ],
        };
        let poly = Polygon {
            rings: vec![exterior.clone()],
        };
        assert_eq!(poly.exterior(), &exterior);
    }

    #[test]
    fn test_polygon_holes() {
        let exterior = Ring {
            points: vec![
                Point { x: 0.0, y: 0.0 },
                Point { x: 4.0, y: 0.0 },
                Point { x: 4.0, y: 4.0 },
                Point { x: 0.0, y: 4.0 },
                Point { x: 0.0, y: 0.0 },
            ],
        };
        let hole = Ring {
            points: vec![
                Point { x: 1.0, y: 1.0 },
                Point { x: 2.0, y: 1.0 },
                Point { x: 2.0, y: 2.0 },
                Point { x: 1.0, y: 2.0 },
                Point { x: 1.0, y: 1.0 },
            ],
        };
        let poly = Polygon {
            rings: vec![exterior, hole.clone()],
        };
        assert_eq!(poly.holes().len(), 1);
        assert_eq!(poly.holes()[0], hole);
    }

    #[test]
    fn test_polygon_area_with_hole() {
        let exterior = Ring {
            points: vec![
                Point { x: 0.0, y: 0.0 },
                Point { x: 4.0, y: 0.0 },
                Point { x: 4.0, y: 4.0 },
                Point { x: 0.0, y: 4.0 },
                Point { x: 0.0, y: 0.0 },
            ],
        };
        let hole = Ring {
            points: vec![
                Point { x: 1.0, y: 1.0 },
                Point { x: 2.0, y: 1.0 },
                Point { x: 2.0, y: 2.0 },
                Point { x: 1.0, y: 2.0 },
                Point { x: 1.0, y: 1.0 },
            ],
        };
        let poly = Polygon {
            rings: vec![exterior, hole],
        };
        // 4x4 = 16, minus 1x1 = 1, = 15
        assert!((poly.area() - 15.0).abs() < 1e-15);
    }

    #[test]
    fn test_geometry_shape_type_polyline() {
        let geom = Geometry::Polyline(Polyline {
            parts: vec![vec![Point { x: 0.0, y: 0.0 }, Point { x: 1.0, y: 1.0 }]],
        });
        assert_eq!(geom.shape_type(), ShapeType::Polyline);
    }

    #[test]
    fn test_geometry_as_polyline_some() {
        let line = Polyline {
            parts: vec![vec![Point { x: 0.0, y: 0.0 }, Point { x: 1.0, y: 1.0 }]],
        };
        let geom = Geometry::Polyline(line.clone());
        assert_eq!(geom.as_polyline(), Some(&line));
    }

    #[test]
    fn test_geometry_is_null() {
        assert!(Geometry::Null.is_null());
        assert!(!Geometry::Point(Point { x: 0.0, y: 0.0 }).is_null());
    }

    // --- MultiPoint tests ---

    #[test]
    fn test_multipoint_bbox() {
        let mp = MultiPoint {
            points: vec![
                Point { x: 1.0, y: 5.0 },
                Point { x: 3.0, y: 2.0 },
                Point { x: 7.0, y: 8.0 },
            ],
        };
        let bb = mp.bbox();
        assert_eq!(bb.x_min, 1.0);
        assert_eq!(bb.y_min, 2.0);
        assert_eq!(bb.x_max, 7.0);
        assert_eq!(bb.y_max, 8.0);
    }

    #[test]
    fn test_multipoint_shape_type() {
        let mp = MultiPoint {
            points: vec![Point { x: 0.0, y: 0.0 }],
        };
        assert_eq!(mp.shape_type(), ShapeType::MultiPoint);
    }

    // --- PointZ tests ---

    #[test]
    fn test_pointz_bbox() {
        let p = PointZ {
            x: 1.0,
            y: 2.0,
            z: 3.0,
            m: None,
        };
        let bb = p.bbox();
        assert_eq!(bb.x_min, 1.0);
        assert_eq!(bb.x_max, 1.0);
        assert_eq!(bb.y_min, 2.0);
        assert_eq!(bb.y_max, 2.0);
    }

    #[test]
    fn test_pointz_shape_type() {
        let p = PointZ {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            m: Some(1.0),
        };
        assert_eq!(p.shape_type(), ShapeType::PointZ);
    }

    // --- PolylineZ tests ---

    #[test]
    fn test_polylinez_bbox() {
        let pl = PolylineZ {
            parts: vec![vec![
                PointZ {
                    x: 1.0,
                    y: 2.0,
                    z: 0.0,
                    m: None,
                },
                PointZ {
                    x: 5.0,
                    y: 8.0,
                    z: 0.0,
                    m: None,
                },
            ]],
        };
        let bb = pl.bbox();
        assert_eq!(bb.x_min, 1.0);
        assert_eq!(bb.y_min, 2.0);
        assert_eq!(bb.x_max, 5.0);
        assert_eq!(bb.y_max, 8.0);
    }

    #[test]
    fn test_polylinez_shape_type() {
        let pl = PolylineZ { parts: vec![] };
        assert_eq!(pl.shape_type(), ShapeType::PolylineZ);
    }

    // --- PolygonZ tests ---

    #[test]
    fn test_polygonz_bbox() {
        let pg = PolygonZ {
            rings: vec![vec![
                PointZ {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    m: None,
                },
                PointZ {
                    x: 4.0,
                    y: 0.0,
                    z: 0.0,
                    m: None,
                },
                PointZ {
                    x: 4.0,
                    y: 4.0,
                    z: 0.0,
                    m: None,
                },
                PointZ {
                    x: 0.0,
                    y: 4.0,
                    z: 0.0,
                    m: None,
                },
            ]],
        };
        let bb = pg.bbox();
        assert_eq!(bb.x_min, 0.0);
        assert_eq!(bb.y_min, 0.0);
        assert_eq!(bb.x_max, 4.0);
        assert_eq!(bb.y_max, 4.0);
    }

    #[test]
    fn test_polygonz_shape_type() {
        let pg = PolygonZ { rings: vec![] };
        assert_eq!(pg.shape_type(), ShapeType::PolygonZ);
    }

    // --- PointM tests ---

    #[test]
    fn test_pointm_bbox() {
        let p = PointM {
            x: 1.0,
            y: 2.0,
            m: 3.0,
        };
        let bb = p.bbox();
        assert_eq!(bb.x_min, 1.0);
        assert_eq!(bb.x_max, 1.0);
        assert_eq!(bb.y_min, 2.0);
        assert_eq!(bb.y_max, 2.0);
    }

    #[test]
    fn test_pointm_shape_type() {
        let p = PointM {
            x: 0.0,
            y: 0.0,
            m: 0.0,
        };
        assert_eq!(p.shape_type(), ShapeType::PointM);
    }

    // --- PolylineM tests ---

    #[test]
    fn test_polylinem_bbox() {
        let pl = PolylineM {
            parts: vec![vec![
                PointM {
                    x: 1.0,
                    y: 2.0,
                    m: 0.0,
                },
                PointM {
                    x: 5.0,
                    y: 8.0,
                    m: 0.0,
                },
            ]],
        };
        let bb = pl.bbox();
        assert_eq!(bb.x_min, 1.0);
        assert_eq!(bb.y_min, 2.0);
        assert_eq!(bb.x_max, 5.0);
        assert_eq!(bb.y_max, 8.0);
    }

    #[test]
    fn test_polylinem_shape_type() {
        let pl = PolylineM { parts: vec![] };
        assert_eq!(pl.shape_type(), ShapeType::PolylineM);
    }

    // --- PolygonM tests ---

    #[test]
    fn test_polygonm_bbox() {
        let pg = PolygonM {
            rings: vec![vec![
                PointM {
                    x: 0.0,
                    y: 0.0,
                    m: 0.0,
                },
                PointM {
                    x: 4.0,
                    y: 0.0,
                    m: 0.0,
                },
                PointM {
                    x: 4.0,
                    y: 4.0,
                    m: 0.0,
                },
                PointM {
                    x: 0.0,
                    y: 4.0,
                    m: 0.0,
                },
            ]],
        };
        let bb = pg.bbox();
        assert_eq!(bb.x_min, 0.0);
        assert_eq!(bb.y_min, 0.0);
        assert_eq!(bb.x_max, 4.0);
        assert_eq!(bb.y_max, 4.0);
    }

    #[test]
    fn test_polygonm_shape_type() {
        let pg = PolygonM { rings: vec![] };
        assert_eq!(pg.shape_type(), ShapeType::PolygonM);
    }

    // --- Geometry enum tests ---

    #[test]
    fn test_geometry_bbox_multipoint() {
        let geom = Geometry::MultiPoint(MultiPoint {
            points: vec![Point { x: 1.0, y: 2.0 }, Point { x: 3.0, y: 4.0 }],
        });
        let bb = geom.bbox().unwrap();
        assert_eq!(bb.x_min, 1.0);
        assert_eq!(bb.y_max, 4.0);
    }

    #[test]
    fn test_geometry_bbox_pointz() {
        let geom = Geometry::PointZ(PointZ {
            x: 10.0,
            y: 20.0,
            z: 30.0,
            m: None,
        });
        let bb = geom.bbox().unwrap();
        assert_eq!(bb.x_min, 10.0);
        assert_eq!(bb.y_min, 20.0);
    }

    #[test]
    fn test_geometry_bbox_polylinez() {
        let geom = Geometry::PolylineZ(PolylineZ {
            parts: vec![vec![
                PointZ {
                    x: 1.0,
                    y: 2.0,
                    z: 0.0,
                    m: None,
                },
                PointZ {
                    x: 3.0,
                    y: 4.0,
                    z: 0.0,
                    m: None,
                },
            ]],
        });
        assert!(geom.bbox().is_some());
    }

    #[test]
    fn test_geometry_bbox_polygonz() {
        let geom = Geometry::PolygonZ(PolygonZ {
            rings: vec![vec![
                PointZ {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    m: None,
                },
                PointZ {
                    x: 1.0,
                    y: 0.0,
                    z: 0.0,
                    m: None,
                },
                PointZ {
                    x: 1.0,
                    y: 1.0,
                    z: 0.0,
                    m: None,
                },
            ]],
        });
        assert!(geom.bbox().is_some());
    }

    #[test]
    fn test_geometry_bbox_pointm() {
        let geom = Geometry::PointM(PointM {
            x: 5.0,
            y: 6.0,
            m: 7.0,
        });
        let bb = geom.bbox().unwrap();
        assert_eq!(bb.x_min, 5.0);
        assert_eq!(bb.y_min, 6.0);
    }

    #[test]
    fn test_geometry_bbox_polylinem() {
        let geom = Geometry::PolylineM(PolylineM {
            parts: vec![vec![
                PointM {
                    x: 1.0,
                    y: 2.0,
                    m: 0.0,
                },
                PointM {
                    x: 3.0,
                    y: 4.0,
                    m: 0.0,
                },
            ]],
        });
        assert!(geom.bbox().is_some());
    }

    #[test]
    fn test_geometry_bbox_polygonm() {
        let geom = Geometry::PolygonM(PolygonM {
            rings: vec![vec![
                PointM {
                    x: 0.0,
                    y: 0.0,
                    m: 0.0,
                },
                PointM {
                    x: 1.0,
                    y: 0.0,
                    m: 0.0,
                },
                PointM {
                    x: 1.0,
                    y: 1.0,
                    m: 0.0,
                },
            ]],
        });
        assert!(geom.bbox().is_some());
    }

    #[test]
    fn test_geometry_bbox_null() {
        assert!(Geometry::Null.bbox().is_none());
    }

    #[test]
    fn test_geometry_as_polygon_some() {
        let poly = Polygon {
            rings: vec![Ring {
                points: vec![
                    Point { x: 0.0, y: 0.0 },
                    Point { x: 1.0, y: 0.0 },
                    Point { x: 1.0, y: 1.0 },
                    Point { x: 0.0, y: 0.0 },
                ],
            }],
        };
        let geom = Geometry::Polygon(poly.clone());
        assert_eq!(geom.as_polygon(), Some(&poly));
    }

    #[test]
    fn test_geometry_as_polygon_none() {
        let geom = Geometry::Null;
        assert_eq!(geom.as_polygon(), None);
    }

    #[test]
    fn test_geometry_as_point_none_for_polyline() {
        let geom = Geometry::Polyline(Polyline {
            parts: vec![vec![Point { x: 0.0, y: 0.0 }]],
        });
        assert_eq!(geom.as_point(), None);
    }

    #[test]
    fn test_geometry_as_polyline_none_for_point() {
        let geom = Geometry::Point(Point { x: 0.0, y: 0.0 });
        assert_eq!(geom.as_polyline(), None);
    }

    #[test]
    fn test_geometry_shape_type_all_variants() {
        assert_eq!(Geometry::Null.shape_type(), ShapeType::Null);
        assert_eq!(
            Geometry::Point(Point { x: 0.0, y: 0.0 }).shape_type(),
            ShapeType::Point
        );
        assert_eq!(
            Geometry::Polyline(Polyline { parts: vec![] }).shape_type(),
            ShapeType::Polyline
        );
        assert_eq!(
            Geometry::Polygon(Polygon { rings: vec![] }).shape_type(),
            ShapeType::Polygon
        );
        assert_eq!(
            Geometry::MultiPoint(MultiPoint { points: vec![] }).shape_type(),
            ShapeType::MultiPoint
        );
        assert_eq!(
            Geometry::PointZ(PointZ {
                x: 0.0,
                y: 0.0,
                z: 0.0,
                m: None
            })
            .shape_type(),
            ShapeType::PointZ
        );
        assert_eq!(
            Geometry::PolylineZ(PolylineZ { parts: vec![] }).shape_type(),
            ShapeType::PolylineZ
        );
        assert_eq!(
            Geometry::PolygonZ(PolygonZ { rings: vec![] }).shape_type(),
            ShapeType::PolygonZ
        );
        assert_eq!(
            Geometry::PointM(PointM {
                x: 0.0,
                y: 0.0,
                m: 0.0
            })
            .shape_type(),
            ShapeType::PointM
        );
        assert_eq!(
            Geometry::PolylineM(PolylineM { parts: vec![] }).shape_type(),
            ShapeType::PolylineM
        );
        assert_eq!(
            Geometry::PolygonM(PolygonM { rings: vec![] }).shape_type(),
            ShapeType::PolygonM
        );
    }
}
