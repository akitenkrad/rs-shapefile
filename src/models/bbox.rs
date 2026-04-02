/// An axis-aligned bounding box defined by min/max x and y coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoundingBox {
    /// Minimum x coordinate (west boundary).
    pub x_min: f64,
    /// Minimum y coordinate (south boundary).
    pub y_min: f64,
    /// Maximum x coordinate (east boundary).
    pub x_max: f64,
    /// Maximum y coordinate (north boundary).
    pub y_max: f64,
}

impl BoundingBox {
    /// Returns `true` if the point (x, y) lies within this bounding box (inclusive).
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x_min && x <= self.x_max && y >= self.y_min && y <= self.y_max
    }

    /// Returns `true` if this bounding box overlaps with `other`.
    pub fn intersects(&self, other: &BoundingBox) -> bool {
        !(other.x_min > self.x_max
            || other.x_max < self.x_min
            || other.y_min > self.y_max
            || other.y_max < self.y_min)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_contains_inside() {
        let bb = BoundingBox {
            x_min: 0.0,
            y_min: 0.0,
            x_max: 10.0,
            y_max: 10.0,
        };
        assert!(bb.contains(5.0, 5.0));
    }

    #[test]
    fn test_contains_on_boundary() {
        let bb = BoundingBox {
            x_min: 0.0,
            y_min: 0.0,
            x_max: 10.0,
            y_max: 10.0,
        };
        assert!(bb.contains(0.0, 0.0));
        assert!(bb.contains(10.0, 10.0));
        assert!(bb.contains(0.0, 10.0));
        assert!(bb.contains(10.0, 0.0));
    }

    #[test]
    fn test_contains_outside() {
        let bb = BoundingBox {
            x_min: 0.0,
            y_min: 0.0,
            x_max: 10.0,
            y_max: 10.0,
        };
        assert!(!bb.contains(-1.0, 5.0));
        assert!(!bb.contains(11.0, 5.0));
        assert!(!bb.contains(5.0, -1.0));
        assert!(!bb.contains(5.0, 11.0));
    }

    #[test]
    fn test_intersects_overlap() {
        let a = BoundingBox {
            x_min: 0.0,
            y_min: 0.0,
            x_max: 10.0,
            y_max: 10.0,
        };
        let b = BoundingBox {
            x_min: 5.0,
            y_min: 5.0,
            x_max: 15.0,
            y_max: 15.0,
        };
        assert!(a.intersects(&b));
        assert!(b.intersects(&a));
    }

    #[test]
    fn test_intersects_no_overlap() {
        let a = BoundingBox {
            x_min: 0.0,
            y_min: 0.0,
            x_max: 5.0,
            y_max: 5.0,
        };
        let b = BoundingBox {
            x_min: 10.0,
            y_min: 10.0,
            x_max: 15.0,
            y_max: 15.0,
        };
        assert!(!a.intersects(&b));
        assert!(!b.intersects(&a));
    }

    #[test]
    fn test_intersects_touching_edge() {
        let a = BoundingBox {
            x_min: 0.0,
            y_min: 0.0,
            x_max: 5.0,
            y_max: 5.0,
        };
        let b = BoundingBox {
            x_min: 5.0,
            y_min: 0.0,
            x_max: 10.0,
            y_max: 5.0,
        };
        assert!(a.intersects(&b));
    }

    #[test]
    fn test_intersects_contained() {
        let outer = BoundingBox {
            x_min: 0.0,
            y_min: 0.0,
            x_max: 20.0,
            y_max: 20.0,
        };
        let inner = BoundingBox {
            x_min: 5.0,
            y_min: 5.0,
            x_max: 10.0,
            y_max: 10.0,
        };
        assert!(outer.intersects(&inner));
        assert!(inner.intersects(&outer));
    }

    #[test]
    fn test_intersects_separated_x() {
        let a = BoundingBox {
            x_min: 0.0,
            y_min: 0.0,
            x_max: 3.0,
            y_max: 10.0,
        };
        let b = BoundingBox {
            x_min: 4.0,
            y_min: 0.0,
            x_max: 7.0,
            y_max: 10.0,
        };
        assert!(!a.intersects(&b));
    }

    #[test]
    fn test_intersects_separated_y() {
        let a = BoundingBox {
            x_min: 0.0,
            y_min: 0.0,
            x_max: 10.0,
            y_max: 3.0,
        };
        let b = BoundingBox {
            x_min: 0.0,
            y_min: 4.0,
            x_max: 10.0,
            y_max: 7.0,
        };
        assert!(!a.intersects(&b));
    }
}
