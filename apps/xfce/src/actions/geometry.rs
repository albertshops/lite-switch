use super::Action;
use crate::x11::{Rect, Result};

fn rect(x: i64, y: i64, width: u32, height: u32) -> Result<Rect> {
    if width == 0 || height == 0 {
        return Err("no usable window area".into());
    }
    Ok(Rect {
        x: x.try_into()?,
        y: y.try_into()?,
        width,
        height,
    })
}
pub fn intersection(a: Rect, b: Rect) -> Option<Rect> {
    let x = i64::from(a.x.max(b.x));
    let y = i64::from(a.y.max(b.y));
    let right = (i64::from(a.x) + i64::from(a.width)).min(i64::from(b.x) + i64::from(b.width));
    let bottom = (i64::from(a.y) + i64::from(a.height)).min(i64::from(b.y) + i64::from(b.height));
    if right <= x || bottom <= y {
        return None;
    }
    rect(
        x,
        y,
        (right - x).try_into().ok()?,
        (bottom - y).try_into().ok()?,
    )
    .ok()
}
impl Action {
    pub fn area(self, usable: Rect, current: Rect) -> Result<Rect> {
        let (mut x, mut y) = (i64::from(usable.x), i64::from(usable.y));
        let (mut w, mut h) = (usable.width, usable.height);
        match self {
            Self::Close => return Err("close is not a geometry action".into()),
            Self::Fill => {}
            Self::LeftHalf => w /= 2,
            Self::RightHalf => {
                x += i64::from(w / 2);
                w -= w / 2;
            }
            Self::TopHalf => h /= 2,
            Self::BottomHalf => {
                y += i64::from(h / 2);
                h -= h / 2;
            }
            Self::Center => {
                w = w.min(current.width);
                h = h.min(current.height);
                x += i64::from((usable.width - w) / 2);
                y += i64::from((usable.height - h) / 2);
            }
            Self::NextOutput | Self::PreviousOutput => {
                return Err("output moves require a destination monitor".into());
            }
        }
        rect(x, y, w, h)
    }
    pub fn anchor(self, area: Rect, width: u32, height: u32) -> Result<Rect> {
        if width > area.width || height > area.height {
            return Err("window constraints exceed usable action area".into());
        }
        let (mut x, mut y) = (i64::from(area.x), i64::from(area.y));
        if self == Self::RightHalf {
            x += i64::from(area.width - width);
        }
        if self == Self::BottomHalf {
            y += i64::from(area.height - height);
        }
        if self == Self::Center {
            x += i64::from((area.width - width) / 2);
            y += i64::from((area.height - height) / 2);
        }
        rect(x, y, width, height)
    }
}
pub fn map_between(
    source: Rect,
    destination: Rect,
    before: Rect,
    width: u32,
    height: u32,
) -> Result<Rect> {
    if source.width == 0
        || source.height == 0
        || width > destination.width
        || height > destination.height
    {
        return Err("invalid output geometry".into());
    }
    fn axis(
        old: i32,
        old_size: u32,
        start: i32,
        span: u32,
        new_start: i32,
        new_span: u32,
        size: u32,
    ) -> Result<i32> {
        let fraction = (2 * (i128::from(old) - i128::from(start)) + i128::from(old_size))
            .clamp(0, 2 * i128::from(span));
        let center =
            i128::from(new_start) + fraction * i128::from(new_span) / (2 * i128::from(span));
        Ok((center - i128::from(size) / 2)
            .clamp(
                i128::from(new_start),
                i128::from(new_start) + i128::from(new_span - size),
            )
            .try_into()?)
    }
    rect(
        i64::from(axis(
            before.x,
            before.width,
            source.x,
            source.width,
            destination.x,
            destination.width,
            width,
        )?),
        i64::from(axis(
            before.y,
            before.height,
            source.y,
            source.height,
            destination.y,
            destination.height,
            height,
        )?),
        width,
        height,
    )
}

#[derive(Debug, Clone)]
pub struct Strut {
    edges: [u32; 4],
    ranges: [[u32; 2]; 4],
    panel: Option<Rect>,
}
impl Strut {
    pub fn from_wire(
        partial: &[u32],
        legacy: &[u32],
        root: Rect,
        panel: Rect,
    ) -> Result<Option<Self>> {
        if partial.is_empty() && legacy.is_empty() {
            return Ok(None);
        }
        if root.width == 0 || root.height == 0 {
            return Err("empty X11 screen".into());
        }
        let (edges, ranges, panel) = if partial.len() == 12 {
            (
                [partial[0], partial[1], partial[2], partial[3]],
                [
                    [partial[4], partial[5]],
                    [partial[6], partial[7]],
                    [partial[8], partial[9]],
                    [partial[10], partial[11]],
                ],
                Some(panel),
            )
        } else if partial.is_empty() && legacy.len() == 4 {
            (
                [legacy[0], legacy[1], legacy[2], legacy[3]],
                [
                    [0, root.height - 1],
                    [0, root.height - 1],
                    [0, root.width - 1],
                    [0, root.width - 1],
                ],
                None,
            )
        } else {
            return Err("invalid panel reservation property".into());
        };
        if edges == [0; 4] {
            return Ok(None);
        }
        for range in &ranges {
            if range[0] > range[1] {
                return Err("invalid panel reservation range".into());
            }
        }
        Ok(Some(Self {
            edges,
            ranges,
            panel,
        }))
    }
}
pub fn usable(output: Rect, root: Rect, struts: &[Strut]) -> Result<Rect> {
    let mut left = i64::from(output.x);
    let mut top = i64::from(output.y);
    let mut right = left + i64::from(output.width);
    let mut bottom = top + i64::from(output.height);
    let root_right = i64::from(root.x) + i64::from(root.width);
    let root_bottom = i64::from(root.y) + i64::from(root.height);
    let overlap = |range: [u32; 2], start: i32, size: u32| {
        i64::from(range[0]) < i64::from(start) + i64::from(size)
            && i64::from(range[1]) + 1 > i64::from(start)
    };
    for strut in struts {
        // Partial reservations belong to the monitors physically hosting the
        // panel, avoiding a lower monitor's top strut swallowing an upper one.
        if strut
            .panel
            .is_some_and(|panel| intersection(panel, output).is_none())
        {
            continue;
        }
        let l = i64::from(root.x) + i64::from(strut.edges[0].min(root.width));
        let r = root_right - i64::from(strut.edges[1].min(root.width));
        let t = i64::from(root.y) + i64::from(strut.edges[2].min(root.height));
        let b = root_bottom - i64::from(strut.edges[3].min(root.height));
        if strut.edges[0] > 0
            && i64::from(output.x) >= i64::from(root.x)
            && i64::from(output.x) < l
            && overlap(strut.ranges[0], output.y, output.height)
        {
            left = left.max(l);
        }
        if strut.edges[1] > 0
            && right <= root_right
            && right > r
            && overlap(strut.ranges[1], output.y, output.height)
        {
            right = right.min(r);
        }
        if strut.edges[2] > 0
            && i64::from(output.y) >= i64::from(root.y)
            && i64::from(output.y) < t
            && overlap(strut.ranges[2], output.x, output.width)
        {
            top = top.max(t);
        }
        if strut.edges[3] > 0
            && bottom <= root_bottom
            && bottom > b
            && overlap(strut.ranges[3], output.x, output.width)
        {
            bottom = bottom.min(b);
        }
    }
    if right <= left || bottom <= top {
        return Err("panel reservations leave no usable output area".into());
    }
    rect(
        left,
        top,
        (right - left).try_into()?,
        (bottom - top).try_into()?,
    )
}

#[derive(Debug, Clone)]
pub struct SizeHints {
    min: [u32; 2],
    max: [u32; 2],
    base: [u32; 2],
    increment: [u32; 2],
    aspect: Option<[[u32; 2]; 2]>,
}
impl SizeHints {
    pub fn from_wire(values: &[u32]) -> Result<Self> {
        let flags = values.first().copied().unwrap_or(0);
        let pair = |index| -> Result<[u32; 2]> {
            Ok([
                *values.get(index).ok_or("incomplete WM_NORMAL_HINTS")?,
                *values.get(index + 1).ok_or("incomplete WM_NORMAL_HINTS")?,
            ])
        };
        let min = if flags & (1 << 4) != 0 {
            pair(5)?.map(|v| v.max(1))
        } else {
            [1; 2]
        };
        let max = if flags & (1 << 5) != 0 {
            pair(7)?
        } else {
            [u32::from(u16::MAX); 2]
        };
        let increment = if flags & (1 << 6) != 0 {
            pair(9)?.map(|v| v.max(1))
        } else {
            [1; 2]
        };
        let base = if flags & (1 << 8) != 0 {
            pair(15)?
        } else if flags & (1 << 4) != 0 {
            min
        } else {
            [0; 2]
        };
        let aspect = if flags & (1 << 7) != 0 {
            let min = pair(11)?;
            let max = pair(13)?;
            if min.contains(&0)
                || max.contains(&0)
                || u64::from(min[0]) * u64::from(max[1]) > u64::from(max[0]) * u64::from(min[1])
            {
                return Err("invalid window aspect constraints".into());
            }
            Some([min, max])
        } else {
            None
        };
        Ok(Self {
            min,
            max,
            base,
            increment,
            aspect,
        })
    }
    pub fn fit(&self, width: u32, height: u32) -> Result<[u32; 2]> {
        let maximum = [
            width.min(self.max[0]).min(u32::from(u16::MAX)),
            height.min(self.max[1]).min(u32::from(u16::MAX)),
        ];
        if maximum[0] < self.min[0] || maximum[1] < self.min[1] {
            return Err("window minimum size exceeds usable action area".into());
        }
        let grid = |value: u32, axis: usize| -> Option<u32> {
            let base = i64::from(self.base[axis]);
            let increment = i64::from(self.increment[axis]);
            u32::try_from(base + (i64::from(value) - base).div_euclid(increment) * increment)
                .ok()
                .filter(|v| *v >= self.min[axis] && *v > 0)
        };
        let mut w = grid(maximum[0], 0).ok_or("window width increments cannot fit usable area")?;
        if self.aspect.is_none() {
            return Ok([
                w,
                grid(maximum[1], 1).ok_or("window height increments cannot fit usable area")?,
            ]);
        }
        let [min_aspect, max_aspect] = self.aspect.unwrap();
        let mut best: Option<[u32; 2]> = None;
        loop {
            let min_h = u64::from(self.min[1])
                .max((u64::from(w) * u64::from(max_aspect[1])).div_ceil(u64::from(max_aspect[0])));
            let max_h = u64::from(maximum[1])
                .min(u64::from(w) * u64::from(min_aspect[1]) / u64::from(min_aspect[0]));
            if min_h <= max_h
                && let Some(h) = grid(max_h.try_into()?, 1)
                && u64::from(h) >= min_h
                && best.is_none_or(|[bw, bh]| {
                    u64::from(w) * u64::from(h) > u64::from(bw) * u64::from(bh)
                })
            {
                best = Some([w, h]);
            }
            let Some(next) = w
                .checked_sub(self.increment[0])
                .filter(|next| *next >= self.min[0])
            else {
                break;
            };
            w = next;
        }
        best.ok_or_else(|| "window aspect/increment constraints cannot fit usable area".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn odd_halves_negative_origins_and_constrained_edge_anchors() {
        let area = Rect {
            x: -641,
            y: 40,
            width: 1281,
            height: 861,
        };
        let left = Action::LeftHalf.area(area, area).unwrap();
        let right = Action::RightHalf.area(area, area).unwrap();
        assert_eq!(left.width + right.width, area.width);
        assert_eq!(right.x, -1);
        assert_eq!(Action::RightHalf.anchor(right, 600, 800).unwrap().x, 40);
        let bottom = Action::BottomHalf.area(area, area).unwrap();
        assert_eq!(bottom.y, 470);
        assert_eq!(bottom.height, 431);
        assert!(
            Action::RightHalf
                .area(
                    Rect {
                        x: i32::MAX,
                        width: 4,
                        ..area
                    },
                    area
                )
                .is_err()
        );
    }
    #[test]
    fn monitor_specific_and_stacked_panel_reservations() {
        let root = Rect {
            x: 0,
            y: 0,
            width: 1280,
            height: 900,
        };
        let left = Rect { width: 640, ..root };
        let right = Rect {
            x: 640,
            width: 640,
            ..root
        };
        let strut = Strut::from_wire(
            &[0, 0, 40, 0, 0, 0, 0, 0, 0, 639, 0, 0],
            &[],
            root,
            Rect { height: 40, ..left },
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            usable(left, root, std::slice::from_ref(&strut)).unwrap(),
            Rect {
                y: 40,
                height: 860,
                ..left
            }
        );
        assert_eq!(usable(right, root, &[strut]).unwrap(), right);
        let root = Rect {
            width: 800,
            height: 1200,
            ..root
        };
        let upper = Rect {
            height: 600,
            ..root
        };
        let lower = Rect {
            y: 600,
            height: 600,
            ..root
        };
        let strut = Strut::from_wire(
            &[0, 0, 640, 0, 0, 0, 0, 0, 0, 799, 0, 0],
            &[],
            root,
            Rect {
                y: 600,
                height: 40,
                ..lower
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            usable(upper, root, std::slice::from_ref(&strut)).unwrap(),
            upper
        );
        assert_eq!(
            usable(lower, root, &[strut]).unwrap(),
            Rect {
                y: 640,
                height: 560,
                ..lower
            }
        );
    }
    #[test]
    fn mapped_output_rectangles_preserve_relative_center_and_fit() {
        let source = Rect {
            x: -1200,
            y: 30,
            width: 1200,
            height: 870,
        };
        let dest = Rect {
            x: 0,
            y: 50,
            width: 800,
            height: 550,
        };
        let original = Rect {
            x: -900,
            y: 200,
            width: 600,
            height: 400,
        };
        let mapped = map_between(source, dest, original, 600, 400).unwrap();
        assert!(intersection(mapped, dest) == Some(mapped));
        assert!(map_between(source, dest, original, 900, 400).is_err());
    }
    #[test]
    fn minimum_maximum_increment_and_aspect_constraints() {
        let mut hints = vec![0; 18];
        hints[0] = (1 << 4) | (1 << 6) | (1 << 8);
        hints[5] = 80;
        hints[6] = 50;
        hints[9] = 6;
        hints[10] = 13;
        hints[15] = 4;
        hints[16] = 4;
        let hints = SizeHints::from_wire(&hints).unwrap();
        assert_eq!(hints.fit(100, 100).unwrap(), [100, 95]);
        assert!(hints.fit(79, 100).is_err());
        let mut aspect = vec![0; 18];
        aspect[0] = 1 << 7;
        aspect[11] = 4;
        aspect[12] = 3;
        aspect[13] = 4;
        aspect[14] = 3;
        assert_eq!(
            SizeHints::from_wire(&aspect)
                .unwrap()
                .fit(100, 100)
                .unwrap(),
            [100, 75]
        );
        aspect[12] = 0;
        assert!(SizeHints::from_wire(&aspect).is_err());
        assert_eq!(
            SizeHints::from_wire(&[]).unwrap().fit(900, 700).unwrap(),
            [900, 700]
        );
    }
}
