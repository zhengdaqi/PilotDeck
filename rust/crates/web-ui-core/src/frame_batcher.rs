//! Port of `ui/src/utils/frameBatcher.ts`: coalesce samples into one animation frame; flush the last on release.

pub trait FrameHost {
    fn request_animation_frame(&mut self) -> u64;
    fn cancel_animation_frame(&mut self, id: u64);
}

pub struct FrameBatcher<T> {
    frame: Option<u64>,
    pending: Option<T>,
}

impl<T> Default for FrameBatcher<T> {
    fn default() -> Self {
        FrameBatcher { frame: None, pending: None }
    }
}

impl<T> FrameBatcher<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&mut self, host: &mut dyn FrameHost) {
        if let Some(f) = self.frame.take() {
            host.cancel_animation_frame(f);
        }
        self.pending = None;
    }

    /// `flush()`; also the animation-frame callback.
    pub fn flush(&mut self, host: &mut dyn FrameHost, apply: &mut dyn FnMut(T)) {
        let next = self.pending.take();
        self.cancel(host);
        if let Some(v) = next {
            apply(v);
        }
    }

    pub fn schedule(&mut self, host: &mut dyn FrameHost, value: T) {
        self.pending = Some(value);
        if self.frame.is_none() {
            self.frame = Some(host.request_animation_frame());
        }
    }

    /// The id of the frame this batcher is waiting for, if any.
    pub fn pending_frame(&self) -> Option<u64> {
        self.frame
    }
}
