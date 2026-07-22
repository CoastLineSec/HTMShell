use crate::ShellHostError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Argb8888Layout {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub byte_len: usize,
}

impl Argb8888Layout {
    pub fn new(width: u32, height: u32) -> Result<Self, ShellHostError> {
        if width == 0 || height == 0 {
            return Err(ShellHostError::InvalidDimensions(
                "width and height must be nonzero".into(),
            ));
        }
        let stride = width
            .checked_mul(4)
            .ok_or_else(|| ShellHostError::InvalidDimensions("stride overflow".into()))?;
        let byte_len_u32 = stride
            .checked_mul(height)
            .ok_or_else(|| ShellHostError::InvalidDimensions("buffer size overflow".into()))?;
        let byte_len = usize::try_from(byte_len_u32)
            .map_err(|_| ShellHostError::InvalidDimensions("buffer size exceeds usize".into()))?;
        if byte_len > i32::MAX as usize {
            return Err(ShellHostError::InvalidDimensions(
                "wl_shm pool would exceed the protocol size limit".into(),
            ));
        }
        Ok(Self {
            width,
            height,
            stride,
            byte_len,
        })
    }
}

/// Converts premultiplied RGBA8 to native-endian `WL_SHM_FORMAT_ARGB8888`.
///
/// The protocol format is the native-endian u32 value `0xAARRGGBB`. On a
/// little-endian host its bytes are therefore B, G, R, A.
pub fn convert_premultiplied_rgba_to_argb8888(
    source: &[u8],
    destination: &mut [u8],
    layout: Argb8888Layout,
) -> Result<(), ShellHostError> {
    let packed_row = usize::try_from(layout.width)
        .ok()
        .and_then(|width| width.checked_mul(4))
        .ok_or_else(|| ShellHostError::InvalidDimensions("row length overflow".into()))?;
    let stride = usize::try_from(layout.stride)
        .map_err(|_| ShellHostError::InvalidDimensions("stride exceeds usize".into()))?;
    let height = usize::try_from(layout.height)
        .map_err(|_| ShellHostError::InvalidDimensions("height exceeds usize".into()))?;
    let expected_source = packed_row
        .checked_mul(height)
        .ok_or_else(|| ShellHostError::InvalidDimensions("source length overflow".into()))?;
    let expected_destination = stride
        .checked_mul(height)
        .ok_or_else(|| ShellHostError::InvalidDimensions("destination length overflow".into()))?;
    if source.len() != expected_source {
        return Err(ShellHostError::Buffer(format!(
            "RGBA source has {} bytes; expected {expected_source}",
            source.len()
        )));
    }
    if destination.len() < expected_destination {
        return Err(ShellHostError::Buffer(format!(
            "ARGB destination has {} bytes; expected at least {expected_destination}",
            destination.len()
        )));
    }
    if stride < packed_row {
        return Err(ShellHostError::InvalidDimensions(
            "stride is shorter than a packed row".into(),
        ));
    }

    for (source_row, destination_row) in source
        .chunks_exact(packed_row)
        .zip(destination.chunks_exact_mut(stride))
    {
        for (rgba, argb) in source_row
            .chunks_exact(4)
            .zip(destination_row[..packed_row].chunks_exact_mut(4))
        {
            let value = u32::from(rgba[3]) << 24
                | u32::from(rgba[0]) << 16
                | u32::from(rgba[1]) << 8
                | u32::from(rgba[2]);
            argb.copy_from_slice(&value.to_ne_bytes());
        }
        destination_row[packed_row..].fill(0);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_colors_use_native_argb8888_with_preserved_premultiplication() {
        let layout = Argb8888Layout::new(6, 1).unwrap();
        let source = [
            255, 0, 0, 255, // red
            0, 255, 0, 255, // green
            0, 0, 255, 255, // blue
            0, 0, 0, 255, // black
            0, 0, 0, 0, // transparent
            64, 32, 16, 128, // premultiplied partial alpha
        ];
        let mut destination = vec![0; layout.byte_len];
        convert_premultiplied_rgba_to_argb8888(&source, &mut destination, layout).unwrap();
        let values: Vec<_> = destination
            .chunks_exact(4)
            .map(|bytes| u32::from_ne_bytes(bytes.try_into().unwrap()))
            .collect();
        assert_eq!(
            values,
            vec![
                0xffff0000, 0xff00ff00, 0xff0000ff, 0xff000000, 0x00000000, 0x80402010,
            ]
        );
    }

    #[test]
    fn invalid_lengths_and_dimension_overflow_are_rejected() {
        let layout = Argb8888Layout::new(2, 2).unwrap();
        assert!(convert_premultiplied_rgba_to_argb8888(&[0; 15], &mut [0; 16], layout).is_err());
        assert!(convert_premultiplied_rgba_to_argb8888(&[0; 16], &mut [0; 15], layout).is_err());
        assert!(Argb8888Layout::new(u32::MAX, 2).is_err());
    }
}
