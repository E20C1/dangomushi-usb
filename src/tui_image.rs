use image::DynamicImage;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use ratatui_image::picker::{Picker, ProtocolType};
use ratatui_image::thread::{ResizeRequest, ResizeResponse, ThreadProtocol};
use ratatui_image::{Resize, StatefulImage};
use std::sync::mpsc::{self, Receiver};
use std::thread;

pub struct ImagePreviewRuntime {
    picker: Picker,
    detection_error: Option<String>,
    before: Option<PreviewSlot>,
    after: Option<PreviewSlot>,
    last_worker_error: Option<String>,
}

impl ImagePreviewRuntime {
    pub fn detect() -> Self {
        match Picker::from_query_stdio() {
            Ok(picker) => Self::from_picker(picker, None),
            Err(error) => Self::from_picker(Picker::halfblocks(), Some(error.to_string())),
        }
    }

    pub fn halfblocks() -> Self {
        Self::from_picker(Picker::halfblocks(), None)
    }

    fn from_picker(picker: Picker, detection_error: Option<String>) -> Self {
        Self {
            picker,
            detection_error,
            before: None,
            after: None,
            last_worker_error: None,
        }
    }

    pub fn protocol_type(&self) -> ProtocolType {
        self.picker.protocol_type()
    }

    pub fn protocol_label(&self) -> &'static str {
        match self.protocol_type() {
            ProtocolType::Halfblocks => "HALFBLOCK",
            ProtocolType::Sixel => "SIXEL",
            ProtocolType::Kitty => "KITTY",
            ProtocolType::Iterm2 => "ITERM2",
        }
    }

    pub fn is_pixel_protocol(&self) -> bool {
        !matches!(self.protocol_type(), ProtocolType::Halfblocks)
    }

    pub fn detection_error(&self) -> Option<&str> {
        self.detection_error.as_deref()
    }

    pub fn last_worker_error(&self) -> Option<&str> {
        self.last_worker_error.as_deref()
    }

    pub fn clear(&mut self) {
        self.before = None;
        self.after = None;
        self.last_worker_error = None;
    }

    pub fn set_pair(&mut self, before: DynamicImage, after: DynamicImage) {
        self.before = Some(PreviewSlot::new(&self.picker, before));
        self.after = Some(PreviewSlot::new(&self.picker, after));
        self.last_worker_error = None;
    }

    pub fn replace_pair(&mut self, before: DynamicImage, after: DynamicImage) {
        match self.before.as_mut() {
            Some(slot) => slot.replace(&self.picker, before),
            None => self.before = Some(PreviewSlot::new(&self.picker, before)),
        }
        match self.after.as_mut() {
            Some(slot) => slot.replace(&self.picker, after),
            None => self.after = Some(PreviewSlot::new(&self.picker, after)),
        }
        self.last_worker_error = None;
    }

    pub fn decode_and_set_pair(
        &mut self,
        before_bytes: &[u8],
        after_bytes: &[u8],
    ) -> Result<(), image::ImageError> {
        let before = image::load_from_memory(before_bytes)?;
        let after = image::load_from_memory(after_bytes)?;
        self.set_pair(before, after);
        Ok(())
    }

    pub fn poll_workers(&mut self) -> bool {
        let mut changed = false;
        if let Some(slot) = self.before.as_mut() {
            changed |= slot.poll(&mut self.last_worker_error);
        }
        if let Some(slot) = self.after.as_mut() {
            changed |= slot.poll(&mut self.last_worker_error);
        }
        changed
    }

    pub fn render_pair(&mut self, frame: &mut Frame, area: Rect) {
        self.poll_workers();

        let panes = if area.width >= 80 {
            Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(area)
        } else {
            Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(area)
        };

        self.render_slot(frame, panes[0], " BEFORE ", true);
        self.render_slot(frame, panes[1], " AFTER ", false);
    }

    fn render_slot(&mut self, frame: &mut Frame, area: Rect, title: &str, before: bool) {
        let block = Block::default().title(title).borders(Borders::ALL);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let slot = if before {
            self.before.as_mut()
        } else {
            self.after.as_mut()
        };

        if let Some(slot) = slot {
            frame.render_stateful_widget(
                StatefulImage::new().resize(Resize::Fit(None)),
                inner,
                &mut slot.protocol,
            );
        } else {
            frame.render_widget(
                Paragraph::new("No preview image"),
                inner,
            );
        }
    }
}

struct PreviewSlot {
    protocol: ThreadProtocol,
    completed: Receiver<Result<ResizeResponse, String>>,
}

impl PreviewSlot {
    fn new(picker: &Picker, image: DynamicImage) -> Self {
        let (request_tx, request_rx) = mpsc::channel::<ResizeRequest>();
        let (completed_tx, completed_rx) = mpsc::channel::<Result<ResizeResponse, String>>();

        thread::spawn(move || {
            while let Ok(request) = request_rx.recv() {
                let completed = request.resize_encode().map_err(|error| error.to_string());
                if completed_tx.send(completed).is_err() {
                    break;
                }
            }
        });

        Self {
            protocol: ThreadProtocol::new(
                request_tx,
                Some(picker.new_resize_protocol(image)),
            ),
            completed: completed_rx,
        }
    }

    fn replace(&mut self, picker: &Picker, image: DynamicImage) {
        self.protocol.replace_protocol(picker.new_resize_protocol(image));
    }

    fn poll(&mut self, last_error: &mut Option<String>) -> bool {
        let mut changed = false;
        while let Ok(result) = self.completed.try_recv() {
            match result {
                Ok(completed) => {
                    if self.protocol.update_resized_protocol(completed) {
                        changed = true;
                    }
                }
                Err(error) => {
                    *last_error = Some(error);
                    changed = true;
                }
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, Rgba, RgbaImage};

    fn sample(pixel: [u8; 4]) -> DynamicImage {
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(2, 2, Rgba(pixel)))
    }

    #[test]
    fn halfblock_runtime_is_explicit_fallback() {
        let runtime = ImagePreviewRuntime::halfblocks();
        assert_eq!(runtime.protocol_type(), ProtocolType::Halfblocks);
        assert_eq!(runtime.protocol_label(), "HALFBLOCK");
        assert!(!runtime.is_pixel_protocol());
    }

    #[test]
    fn preview_pair_can_be_replaced_without_changing_protocol_choice() {
        let mut runtime = ImagePreviewRuntime::halfblocks();
        runtime.set_pair(sample([255, 0, 0, 255]), sample([0, 255, 0, 255]));
        runtime.replace_pair(sample([0, 0, 255, 255]), sample([255, 255, 255, 255]));
        assert_eq!(runtime.protocol_type(), ProtocolType::Halfblocks);
        assert!(runtime.last_worker_error().is_none());
    }
}
