use crate::types::Position;

// First contact along a segment, including initial overlap. Fractions are in [0, 1].
pub fn circle_time(start: Position, end: Position, center: Position, radius: f32) -> Option<f32> {
    let x = start.x - center.x;
    let y = start.y - center.y;
    let c = x * x + y * y - radius * radius;
    if c <= 0.0 {
        return Some(0.0);
    }
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let a = dx * dx + dy * dy;
    if a <= f32::EPSILON {
        return None;
    }
    let b = x * dx + y * dy;
    if b >= 0.0 {
        return None;
    }
    let discriminant = b * b - a * c;
    if discriminant < 0.0 {
        return None;
    }
    let t = (-b - discriminant.sqrt()) / a;
    (0.0..=1.0).contains(&t).then_some(t)
}

pub fn box_time(start: Position, end: Position, min: Position, max: Position) -> Option<f32> {
    let mut entry: f32 = 0.0;
    let mut exit: f32 = 1.0;
    for (origin, delta, lower, upper) in [
        (start.x, end.x - start.x, min.x, max.x),
        (start.y, end.y - start.y, min.y, max.y),
    ] {
        if delta.abs() <= f32::EPSILON {
            if origin < lower || origin > upper {
                return None;
            }
        } else {
            let a = (lower - origin) / delta;
            let b = (upper - origin) / delta;
            entry = entry.max(a.min(b));
            exit = exit.min(a.max(b));
            if entry > exit {
                return None;
            }
        }
    }
    Some(entry)
}
