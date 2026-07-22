use crate::ShellHostError;
use crate::pixel::{Argb8888Layout, convert_premultiplied_rgba_to_argb8888};
use memmap2::{MmapMut, MmapOptions};
use rustix::fs::{MemfdFlags, memfd_create};
use std::fs::File;
use std::os::fd::AsFd;
use std::time::Instant;
use wayland_client::{
    QueueHandle,
    protocol::{wl_buffer, wl_shm, wl_shm_pool},
};

const BUFFER_COUNT: usize = 2;
const MAX_RETIRED_BUFFERS: usize = 4;
const MAX_TOTAL_MAPPED_BYTES: usize = 256 * 1024 * 1024;

#[derive(Debug, Clone, Copy)]
pub(crate) struct BufferData {
    pub(crate) id: u64,
}

struct BufferSlot {
    id: u64,
    proxy: wl_buffer::WlBuffer,
    _file: File,
    mapping: MmapMut,
    busy: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct BufferPoolStats {
    pub allocations: u64,
    pub reallocations: u64,
    pub releases: u64,
    pub skipped_no_free_buffer: u64,
    pub total_mapped_bytes: usize,
}

pub(crate) struct ShmBufferPool {
    slots: Vec<BufferSlot>,
    retired: Vec<BufferSlot>,
    layout: Option<Argb8888Layout>,
    next_id: u64,
    stats: BufferPoolStats,
}

impl ShmBufferPool {
    pub(crate) fn new() -> Self {
        Self {
            slots: Vec::new(),
            retired: Vec::new(),
            layout: None,
            next_id: 1,
            stats: BufferPoolStats::default(),
        }
    }

    pub(crate) fn has_free(&self) -> bool {
        self.slots.iter().any(|slot| !slot.busy)
    }

    pub(crate) fn all_released(&self) -> bool {
        self.slots
            .iter()
            .chain(&self.retired)
            .all(|slot| !slot.busy)
    }

    pub(crate) fn ensure_size<State>(
        &mut self,
        shm: &wl_shm::WlShm,
        qh: &QueueHandle<State>,
        width: u32,
        height: u32,
    ) -> Result<bool, ShellHostError>
    where
        State: wayland_client::Dispatch<wl_buffer::WlBuffer, BufferData>
            + wayland_client::Dispatch<wl_shm_pool::WlShmPool, ()>
            + 'static,
    {
        let layout = Argb8888Layout::new(width, height)?;
        if self.layout == Some(layout) {
            return Ok(true);
        }
        let retiring_busy = self.slots.iter().filter(|slot| slot.busy).count();
        if self.retired.len().saturating_add(retiring_busy) > MAX_RETIRED_BUFFERS {
            return Ok(false);
        }
        let new_total = layout
            .byte_len
            .checked_mul(BUFFER_COUNT)
            .ok_or_else(|| ShellHostError::Buffer("total mapped bytes overflow".into()))?;
        let retired_total = self
            .retired
            .iter()
            .chain(self.slots.iter().filter(|slot| slot.busy))
            .try_fold(0usize, |total, slot| total.checked_add(slot.mapping.len()))
            .ok_or_else(|| ShellHostError::Buffer("retired mapped bytes overflow".into()))?;
        let total = new_total
            .checked_add(retired_total)
            .ok_or_else(|| ShellHostError::Buffer("total mapped bytes overflow".into()))?;
        if total > MAX_TOTAL_MAPPED_BYTES {
            return Err(ShellHostError::Buffer(format!(
                "two-buffer pool requires {total} bytes; limit is {MAX_TOTAL_MAPPED_BYTES}"
            )));
        }
        let was_allocated = !self.slots.is_empty();
        for slot in self.slots.drain(..) {
            if slot.busy {
                self.retired.push(slot);
            } else {
                slot.proxy.destroy();
            }
        }
        for _ in 0..BUFFER_COUNT {
            self.slots.push(create_slot(shm, qh, layout, self.next_id)?);
            self.next_id = self.next_id.saturating_add(1);
        }
        self.layout = Some(layout);
        self.stats.allocations = self.stats.allocations.saturating_add(BUFFER_COUNT as u64);
        if was_allocated {
            self.stats.reallocations = self.stats.reallocations.saturating_add(1);
        }
        self.refresh_mapped_bytes();
        Ok(true)
    }

    pub(crate) fn acquire_and_write(
        &mut self,
        rgba: &[u8],
    ) -> Result<Option<(u64, wl_buffer::WlBuffer, u64)>, ShellHostError> {
        let Some(layout) = self.layout else {
            return Err(ShellHostError::Buffer(
                "buffer pool has not been configured".into(),
            ));
        };
        let Some(slot) = self.slots.iter_mut().find(|slot| !slot.busy) else {
            self.stats.skipped_no_free_buffer = self.stats.skipped_no_free_buffer.saturating_add(1);
            return Ok(None);
        };
        let started = Instant::now();
        convert_premultiplied_rgba_to_argb8888(rgba, &mut slot.mapping, layout)?;
        let conversion_us = started.elapsed().as_micros().min(u128::from(u64::MAX)) as u64;
        slot.busy = true;
        Ok(Some((slot.id, slot.proxy.clone(), conversion_us)))
    }

    pub(crate) fn release(&mut self, id: u64) {
        if let Some(slot) = self.slots.iter_mut().find(|slot| slot.id == id)
            && slot.busy
        {
            slot.busy = false;
            self.stats.releases = self.stats.releases.saturating_add(1);
            return;
        }
        if let Some(index) = self.retired.iter().position(|slot| slot.id == id) {
            let mut slot = self.retired.swap_remove(index);
            if slot.busy {
                slot.busy = false;
                self.stats.releases = self.stats.releases.saturating_add(1);
            }
            slot.proxy.destroy();
            self.refresh_mapped_bytes();
        }
    }

    pub(crate) fn stats(&self) -> BufferPoolStats {
        self.stats
    }

    pub(crate) fn destroy_all(&mut self) {
        for slot in self.slots.drain(..).chain(self.retired.drain(..)) {
            slot.proxy.destroy();
        }
        self.layout = None;
        self.stats.total_mapped_bytes = 0;
    }

    fn refresh_mapped_bytes(&mut self) {
        self.stats.total_mapped_bytes = self
            .slots
            .iter()
            .chain(&self.retired)
            .map(|slot| slot.mapping.len())
            .sum();
    }
}

fn create_slot<State>(
    shm: &wl_shm::WlShm,
    qh: &QueueHandle<State>,
    layout: Argb8888Layout,
    id: u64,
) -> Result<BufferSlot, ShellHostError>
where
    State: wayland_client::Dispatch<wl_buffer::WlBuffer, BufferData>
        + wayland_client::Dispatch<wl_shm_pool::WlShmPool, ()>
        + 'static,
{
    let fd = memfd_create("htmshell-live", MemfdFlags::CLOEXEC)
        .map_err(|error| ShellHostError::io("create shared-memory file", error.into()))?;
    let file = File::from(fd);
    file.set_len(layout.byte_len as u64)
        .map_err(|error| ShellHostError::io("size shared-memory file", error))?;
    // SAFETY: the mapping covers the file's fixed current length, the File stays
    // owned by BufferSlot for the mapping lifetime, and resizing is never done.
    let mapping = unsafe {
        MmapOptions::new()
            .len(layout.byte_len)
            .map_mut(&file)
            .map_err(|error| ShellHostError::io("map shared-memory file", error))?
    };
    let pool = shm.create_pool(file.as_fd(), layout.byte_len as i32, qh, ());
    let proxy = pool.create_buffer(
        0,
        layout.width as i32,
        layout.height as i32,
        layout.stride as i32,
        wl_shm::Format::Argb8888,
        qh,
        BufferData { id },
    );
    pool.destroy();
    Ok(BufferSlot {
        id,
        proxy,
        _file: file,
        mapping,
        busy: false,
    })
}

#[cfg(test)]
mod tests {
    use super::MAX_RETIRED_BUFFERS;

    #[derive(Debug)]
    struct Model {
        busy: [bool; 2],
        retired_busy: usize,
        size: Option<(u32, u32)>,
    }

    impl Model {
        fn acquire(&mut self) -> Option<usize> {
            let index = self.busy.iter().position(|busy| !busy)?;
            self.busy[index] = true;
            Some(index)
        }

        fn release_active(&mut self, index: usize) {
            self.busy[index] = false;
        }

        fn resize(&mut self, size: (u32, u32)) -> bool {
            let retiring = self.busy.iter().filter(|busy| **busy).count();
            if self.retired_busy.saturating_add(retiring) > MAX_RETIRED_BUFFERS {
                return false;
            }
            self.retired_busy = self.retired_busy.saturating_add(retiring);
            self.busy = [false; 2];
            self.size = Some(size);
            true
        }

        fn release_retired(&mut self) {
            self.retired_busy = self.retired_busy.saturating_sub(1);
        }
    }

    #[test]
    fn two_buffer_model_never_reuses_busy_storage() {
        let mut model = Model {
            busy: [false; 2],
            retired_busy: 0,
            size: Some((100, 100)),
        };
        assert_eq!(model.acquire(), Some(0));
        assert_eq!(model.acquire(), Some(1));
        assert_eq!(model.acquire(), None);
        model.release_active(0);
        assert_eq!(model.acquire(), Some(0));
    }

    #[test]
    fn resize_retires_busy_storage_without_reusing_it() {
        let mut model = Model {
            busy: [false; 2],
            retired_busy: 0,
            size: Some((100, 100)),
        };
        model.acquire().unwrap();
        assert!(model.resize((200, 200)));
        assert_eq!(model.size, Some((200, 200)));
        assert_eq!(model.retired_busy, 1);
        assert_eq!(model.acquire(), Some(0));
        model.release_retired();
        assert_eq!(model.retired_busy, 0);
    }

    #[test]
    fn retired_storage_is_bounded() {
        let mut model = Model {
            busy: [true; 2],
            retired_busy: MAX_RETIRED_BUFFERS - 1,
            size: Some((100, 100)),
        };
        assert!(!model.resize((200, 200)));
        assert_eq!(model.size, Some((100, 100)));
        assert_eq!(model.retired_busy, MAX_RETIRED_BUFFERS - 1);
    }
}
