use crate::datetime::DateTime;

use geo::prelude::Contains;

use geo_types::PointsIter;

use std::fmt;

#[derive(Debug, Default, Clone)]
pub struct Filter {
    rect: Option<geo::Rect<f64>>,
    start: Option<DateTime>,
    end: Option<DateTime>,
}

impl Eq for Filter {}

impl PartialEq for Filter {
    fn eq(&self, other: &Self) -> bool {
        if self.start != other.start {
            return false;
        }

        if self.end != other.end {
            return false;
        }

        match (self.rect, other.rect) {
            (Some(s), Some(o)) => {
                Self::coord_eq(s.min(), o.min())
                    && Self::coord_eq(s.max(), o.max())
            }
            (None, None) => true,
            _ => false,
        }
    }
}

impl Filter {
    fn coord_eq(a: geo::Coordinate<f64>, b: geo::Coordinate<f64>) -> bool {
        a.x.to_bits() == b.x.to_bits() && a.y.to_bits() == b.y.to_bits()
    }

    pub fn end(mut self, end: DateTime) -> Self {
        self.end = Some(end);
        self
    }

    pub fn start(mut self, start: DateTime) -> Self {
        self.start = Some(start);
        self
    }

    pub fn rect(
        mut self,
        min_lat: f64,
        min_lng: f64,
        max_lat: f64,
        max_lng: f64,
    ) -> Self {
        let min = geo::Coordinate {
            y: min_lat,
            x: min_lng,
        };

        let max = geo::Coordinate {
            y: max_lat,
            x: max_lng,
        };

        self.rect = Some(geo::Rect::new(min, max));
        self
    }
}

enum GeometryIter<'a> {
    One(std::iter::Once<Point>),
    Many(PathIter<'a>),
}

impl<'a> std::iter::Iterator for GeometryIter<'a> {
    type Item = Point;

    fn next(&mut self) -> Option<Point> {
        match self {
            GeometryIter::One(o) => o.next(),
            GeometryIter::Many(p) => p.next(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Geometry {
    Point(Point),
    Path(Path),
}

impl Geometry {
    pub fn matches(&self, filter: &Filter) -> bool {
        match self {
            Geometry::Point(p) => p.matches(filter),
            Geometry::Path(p) => p.matches(filter),
        }
    }

    pub fn len(&self) -> usize {
        match self {
            Geometry::Point(_) => 1,
            Geometry::Path(p) => p.len(),
        }
    }

    pub fn iter<'a>(&'a self) -> impl Iterator<Item = Point> + 'a {
        match self {
            Geometry::Point(p) => GeometryIter::One(std::iter::once(*p)),
            Geometry::Path(p) => GeometryIter::Many(p.iter()),
        }
    }
}

impl From<Path> for Geometry {
    fn from(p: Path) -> Self {
        Geometry::Path(p)
    }
}

impl From<Point> for Geometry {
    fn from(p: Point) -> Self {
        Geometry::Point(p)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Point {
    position: geo::Point<f64>,
    time: DateTime,
}

impl Point {
    pub fn new(lat: f64, lng: f64, time: DateTime) -> Self {
        Self {
            position: geo::Point::new(lng, lat),
            time,
        }
    }

    pub fn matches(&self, filter: &Filter) -> bool {
        if let Some(start) = filter.start {
            if self.time < start {
                return false;
            }
        }

        if let Some(end) = filter.end {
            if self.time > end {
                return false;
            }
        }

        if let Some(rect) = filter.rect {
            if !rect.contains(&self.position) {
                return false;
            }
        }

        true
    }

    pub fn latitude(&self) -> f64 {
        self.position.lat()
    }

    pub fn longitude(&self) -> f64 {
        self.position.lng()
    }

    pub fn time(&self) -> DateTime {
        self.time
    }
}

pub struct PathIter<'a> {
    inner: std::iter::Zip<PointsIter<'a, f64>, std::slice::Iter<'a, DateTime>>,
}

impl<'a> fmt::Debug for PathIter<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "PathIter {{ ... }}")
    }
}

impl<'a> std::iter::Iterator for PathIter<'a> {
    type Item = Point;

    fn next(&mut self) -> Option<Point> {
        self.inner.next().map(|(position, time)| Point {
            position,
            time: *time,
        })
    }
}

#[derive(Debug, Clone)]
pub struct Path {
    positions: geo::LineString<f64>,
    times: Vec<DateTime>,
}

impl Path {
    pub fn from_iter<I>(points: I) -> Self
    where
        I: IntoIterator<Item = Point>,
    {
        let mut times = Vec::new();
        let mut gpoints = Vec::new();

        for point in points.into_iter() {
            times.push(point.time);
            gpoints.push(geo::Point::new(point.latitude(), point.longitude()));
        }

        Self {
            times,
            positions: geo::LineString::from(gpoints),
        }
    }

    pub fn matches(&self, filter: &Filter) -> bool {
        // TODO: Might be more efficient to use the intersects method.
        for (time, position) in
            self.times.iter().zip(self.positions.points_iter())
        {
            let point = Point {
                position,
                time: *time,
            };

            if point.matches(filter) {
                return true;
            }
        }

        false
    }

    pub fn iter(&self) -> PathIter {
        PathIter {
            inner: self.positions.points_iter().zip(self.times.iter()),
        }
    }

    pub fn len(&self) -> usize {
        self.times.len()
    }
}
