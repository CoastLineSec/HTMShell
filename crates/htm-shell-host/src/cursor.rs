//! One bounded, host-owned arrow. No file, theme, font, or package authority.

pub(crate) const SIZE: u32 = 24;
pub(crate) const HOTSPOT: (i32, i32) = (2, 1);

pub(crate) fn arrow_pixels() -> Vec<u8> {
    const OUTER: [(f32, f32); 7] = [
        (2.0, 1.0),
        (2.0, 19.0),
        (7.0, 14.0),
        (11.0, 22.0),
        (14.0, 20.0),
        (10.0, 13.0),
        (17.0, 13.0),
    ];
    const INNER: [(f32, f32); 7] = [
        (3.0, 4.0),
        (3.0, 16.0),
        (7.3, 11.7),
        (11.5, 20.5),
        (12.5, 20.0),
        (8.4, 12.0),
        (14.0, 12.0),
    ];
    let mut pixels = vec![0; (SIZE * SIZE * 4) as usize];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let point = (x as f32 + 0.5, y as f32 + 0.5);
            if inside(point, &OUTER) {
                let value = if inside(point, &INNER) { 255 } else { 0 };
                let offset = ((y * SIZE + x) * 4) as usize;
                pixels[offset..offset + 4].copy_from_slice(&[value, value, value, 255]);
            }
        }
    }
    pixels
}

fn inside(point: (f32, f32), polygon: &[(f32, f32)]) -> bool {
    let mut result = false;
    let mut prior = polygon[polygon.len() - 1];
    for &next in polygon {
        if (prior.1 > point.1) != (next.1 > point.1)
            && point.0 < (next.0 - prior.0) * (point.1 - prior.1) / (next.1 - prior.1) + prior.0
        {
            result = !result;
        }
        prior = next;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrow_is_bounded_premultiplied_and_has_a_visible_tip() {
        let pixels = arrow_pixels();
        assert_eq!(pixels.len(), (SIZE * SIZE * 4) as usize);
        assert_eq!(HOTSPOT, (2, 1));
        assert_eq!(pixels[((SIZE + 2) * 4 + 3) as usize], 255);
        assert!(pixels.as_chunks::<4>().0.contains(&[255; 4]));
        assert!(pixels.as_chunks::<4>().0.contains(&[0, 0, 0, 255]));
        for pixel in pixels.as_chunks::<4>().0 {
            assert!(pixel[..3].iter().all(|channel| *channel <= pixel[3]));
        }
        assert_eq!(&pixels[..4], &[0; 4]);
    }
}
