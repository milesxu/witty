use serde::{Deserialize, Serialize};

use crate::{CellPoint, MouseEncodingMode, MouseTrackingMode, TerminalMouseModes};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TerminalMouseEvent {
    pub kind: MouseEventKind,
    pub button: MouseButtonCode,
    pub cell: CellPoint,
    pub pixel: Option<PixelMousePosition>,
    pub modifiers: MouseModifiers,
}

impl TerminalMouseEvent {
    pub const fn new(kind: MouseEventKind, button: MouseButtonCode, cell: CellPoint) -> Self {
        Self {
            kind,
            button,
            cell,
            pixel: None,
            modifiers: MouseModifiers::NONE,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum MouseEventKind {
    Press,
    Release,
    Motion,
    Wheel,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum FocusEventKind {
    In,
    Out,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum MouseButtonCode {
    #[default]
    None,
    Left,
    Middle,
    Right,
    WheelUp,
    WheelDown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PixelMousePosition {
    pub x: u16,
    pub y: u16,
}

impl PixelMousePosition {
    pub const fn new(x: u16, y: u16) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct MouseModifiers {
    pub shift: bool,
    pub alt: bool,
    pub control: bool,
}

impl MouseModifiers {
    pub const NONE: Self = Self {
        shift: false,
        alt: false,
        control: false,
    };

    fn xterm_bits(self) -> u16 {
        let mut bits = 0;
        if self.shift {
            bits += 4;
        }
        if self.alt {
            bits += 8;
        }
        if self.control {
            bits += 16;
        }
        bits
    }
}

impl TerminalMouseModes {
    pub fn reports_mouse(self) -> bool {
        self.tracking != MouseTrackingMode::None
    }

    pub fn reports_focus(self) -> bool {
        self.focus_events
    }
}

pub fn encode_terminal_focus_event(
    event: FocusEventKind,
    modes: TerminalMouseModes,
) -> Option<Vec<u8>> {
    if !modes.reports_focus() {
        return None;
    }

    Some(match event {
        FocusEventKind::In => b"\x1b[I".to_vec(),
        FocusEventKind::Out => b"\x1b[O".to_vec(),
    })
}

pub fn encode_terminal_mouse_event(
    event: TerminalMouseEvent,
    modes: TerminalMouseModes,
) -> Option<Vec<u8>> {
    if !tracking_reports_event(modes.tracking, event) {
        return None;
    }

    match modes.encoding {
        MouseEncodingMode::X10 => encode_x10_mouse_event(event),
        MouseEncodingMode::Utf8 => encode_utf8_mouse_event(event),
        MouseEncodingMode::Urxvt => encode_urxvt_mouse_event(event),
        MouseEncodingMode::Sgr | MouseEncodingMode::SgrPixels => Some(encode_sgr_mouse_event(
            event,
            mouse_event_position(event, modes.encoding)?,
        )),
    }
}

fn tracking_reports_event(tracking: MouseTrackingMode, event: TerminalMouseEvent) -> bool {
    match tracking {
        MouseTrackingMode::None => false,
        MouseTrackingMode::X10 => {
            matches!(event.kind, MouseEventKind::Press | MouseEventKind::Wheel)
        }
        MouseTrackingMode::Normal => matches!(
            event.kind,
            MouseEventKind::Press | MouseEventKind::Release | MouseEventKind::Wheel
        ),
        MouseTrackingMode::ButtonEvent => {
            matches!(
                event.kind,
                MouseEventKind::Press | MouseEventKind::Release | MouseEventKind::Wheel
            ) || (event.kind == MouseEventKind::Motion && event.button != MouseButtonCode::None)
        }
        MouseTrackingMode::AnyEvent => true,
    }
}

fn encode_sgr_mouse_event(event: TerminalMouseEvent, position: MouseProtocolPosition) -> Vec<u8> {
    let cb =
        mouse_event_code(event, true).expect("event should be encodable after tracking filter");
    let final_byte = if event.kind == MouseEventKind::Release {
        'm'
    } else {
        'M'
    };

    format!("\x1b[<{};{};{}{}", cb, position.x, position.y, final_byte).into_bytes()
}

fn encode_x10_mouse_event(event: TerminalMouseEvent) -> Option<Vec<u8>> {
    let cb = x10_encoded_byte(mouse_event_code(event, false)?)?;
    let position = mouse_event_position(event, MouseEncodingMode::X10)?;
    let x = x10_encoded_byte(position.x)?;
    let y = x10_encoded_byte(position.y)?;

    Some(vec![0x1b, b'[', b'M', cb, x, y])
}

fn encode_utf8_mouse_event(event: TerminalMouseEvent) -> Option<Vec<u8>> {
    let cb = utf8_encoded_value(mouse_event_code(event, false)?)?;
    let position = mouse_event_position(event, MouseEncodingMode::Utf8)?;
    let x = utf8_encoded_value(position.x)?;
    let y = utf8_encoded_value(position.y)?;

    let mut bytes = b"\x1b[M".to_vec();
    bytes.extend_from_slice(&cb);
    bytes.extend_from_slice(&x);
    bytes.extend_from_slice(&y);
    Some(bytes)
}

fn encode_urxvt_mouse_event(event: TerminalMouseEvent) -> Option<Vec<u8>> {
    let cb = mouse_event_code(event, false)?.checked_add(32)?;
    let position = mouse_event_position(event, MouseEncodingMode::Urxvt)?;

    Some(format!("\x1b[{cb};{};{}M", position.x, position.y).into_bytes())
}

fn mouse_event_code(event: TerminalMouseEvent, sgr: bool) -> Option<u16> {
    let base = match event.kind {
        MouseEventKind::Wheel => match event.button {
            MouseButtonCode::WheelUp => 64,
            MouseButtonCode::WheelDown => 65,
            _ => return None,
        },
        MouseEventKind::Release if !sgr => 3,
        _ => match event.button {
            MouseButtonCode::None => 3,
            MouseButtonCode::Left => 0,
            MouseButtonCode::Middle => 1,
            MouseButtonCode::Right => 2,
            MouseButtonCode::WheelUp | MouseButtonCode::WheelDown => return None,
        },
    };

    let motion = if event.kind == MouseEventKind::Motion {
        32
    } else {
        0
    };

    Some(base + motion + event.modifiers.xterm_bits())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct MouseProtocolPosition {
    x: u16,
    y: u16,
}

fn mouse_event_position(
    event: TerminalMouseEvent,
    encoding: MouseEncodingMode,
) -> Option<MouseProtocolPosition> {
    match encoding {
        MouseEncodingMode::SgrPixels => {
            let pixel = event.pixel?;
            Some(MouseProtocolPosition {
                x: pixel.x.checked_add(1)?,
                y: pixel.y.checked_add(1)?,
            })
        }
        MouseEncodingMode::X10
        | MouseEncodingMode::Utf8
        | MouseEncodingMode::Urxvt
        | MouseEncodingMode::Sgr => Some(MouseProtocolPosition {
            x: event.cell.col.checked_add(1)?,
            y: event.cell.row.checked_add(1)?,
        }),
    }
}

fn utf8_encoded_value(value: u16) -> Option<Vec<u8>> {
    let encoded = u32::from(value.checked_add(32)?);
    let ch = char::from_u32(encoded)?;
    let mut bytes = [0; 4];
    Some(ch.encode_utf8(&mut bytes).as_bytes().to_vec())
}

fn x10_encoded_byte(value: u16) -> Option<u8> {
    let encoded = value.checked_add(32)?;
    u8::try_from(encoded).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sgr_modes(tracking: MouseTrackingMode) -> TerminalMouseModes {
        TerminalMouseModes {
            tracking,
            encoding: MouseEncodingMode::Sgr,
            focus_events: false,
            alternate_scroll: false,
        }
    }

    fn x10_modes(tracking: MouseTrackingMode) -> TerminalMouseModes {
        TerminalMouseModes {
            tracking,
            encoding: MouseEncodingMode::X10,
            focus_events: false,
            alternate_scroll: false,
        }
    }

    fn utf8_modes(tracking: MouseTrackingMode) -> TerminalMouseModes {
        TerminalMouseModes {
            tracking,
            encoding: MouseEncodingMode::Utf8,
            focus_events: false,
            alternate_scroll: false,
        }
    }

    fn urxvt_modes(tracking: MouseTrackingMode) -> TerminalMouseModes {
        TerminalMouseModes {
            tracking,
            encoding: MouseEncodingMode::Urxvt,
            focus_events: false,
            alternate_scroll: false,
        }
    }

    fn event(
        kind: MouseEventKind,
        button: MouseButtonCode,
        row: u16,
        col: u16,
    ) -> TerminalMouseEvent {
        TerminalMouseEvent::new(kind, button, CellPoint::new(row, col))
    }

    #[test]
    fn sgr_encoder_emits_press_release_motion_and_wheel() {
        let modes = sgr_modes(MouseTrackingMode::ButtonEvent);

        assert_eq!(
            encode_terminal_mouse_event(
                event(MouseEventKind::Press, MouseButtonCode::Left, 0, 0),
                modes
            ),
            Some(b"\x1b[<0;1;1M".to_vec())
        );
        assert_eq!(
            encode_terminal_mouse_event(
                event(MouseEventKind::Release, MouseButtonCode::Left, 0, 0),
                modes
            ),
            Some(b"\x1b[<0;1;1m".to_vec())
        );
        assert_eq!(
            encode_terminal_mouse_event(
                event(MouseEventKind::Motion, MouseButtonCode::Left, 1, 2),
                modes
            ),
            Some(b"\x1b[<32;3;2M".to_vec())
        );
        assert_eq!(
            encode_terminal_mouse_event(
                event(MouseEventKind::Wheel, MouseButtonCode::WheelDown, 1, 2),
                modes
            ),
            Some(b"\x1b[<65;3;2M".to_vec())
        );
    }

    #[test]
    fn sgr_encoder_applies_mouse_modifier_bits() {
        let mut event = event(MouseEventKind::Press, MouseButtonCode::Right, 0, 0);
        event.modifiers = MouseModifiers {
            shift: true,
            alt: true,
            control: true,
        };

        assert_eq!(
            encode_terminal_mouse_event(event, sgr_modes(MouseTrackingMode::Normal)),
            Some(b"\x1b[<30;1;1M".to_vec())
        );
    }

    #[test]
    fn sgr_pixel_encoder_uses_pixel_position_when_available() {
        let mut event = event(MouseEventKind::Press, MouseButtonCode::Left, 9, 9);
        event.pixel = Some(PixelMousePosition::new(20, 40));
        let modes = TerminalMouseModes {
            tracking: MouseTrackingMode::Normal,
            encoding: MouseEncodingMode::SgrPixels,
            focus_events: false,
            alternate_scroll: false,
        };

        assert_eq!(
            encode_terminal_mouse_event(event, modes),
            Some(b"\x1b[<0;21;41M".to_vec())
        );

        event.pixel = None;
        assert_eq!(encode_terminal_mouse_event(event, modes), None);
    }

    #[test]
    fn x10_encoder_emits_legacy_byte_form() {
        let modes = x10_modes(MouseTrackingMode::Normal);

        assert_eq!(
            encode_terminal_mouse_event(
                event(MouseEventKind::Press, MouseButtonCode::Left, 0, 0),
                modes
            ),
            Some(vec![0x1b, b'[', b'M', 32, 33, 33])
        );
        assert_eq!(
            encode_terminal_mouse_event(
                event(MouseEventKind::Release, MouseButtonCode::Right, 0, 0),
                modes
            ),
            Some(vec![0x1b, b'[', b'M', 35, 33, 33])
        );
        assert_eq!(
            encode_terminal_mouse_event(
                event(MouseEventKind::Wheel, MouseButtonCode::WheelUp, 0, 0),
                modes
            ),
            Some(vec![0x1b, b'[', b'M', 96, 33, 33])
        );
    }

    #[test]
    fn tracking_modes_filter_unreported_events() {
        assert_eq!(
            encode_terminal_mouse_event(
                event(MouseEventKind::Release, MouseButtonCode::Left, 0, 0),
                sgr_modes(MouseTrackingMode::X10)
            ),
            None
        );
        assert_eq!(
            encode_terminal_mouse_event(
                event(MouseEventKind::Motion, MouseButtonCode::Left, 0, 0),
                sgr_modes(MouseTrackingMode::Normal)
            ),
            None
        );
        assert_eq!(
            encode_terminal_mouse_event(
                event(MouseEventKind::Motion, MouseButtonCode::None, 0, 0),
                sgr_modes(MouseTrackingMode::ButtonEvent)
            ),
            None
        );
        assert_eq!(
            encode_terminal_mouse_event(
                event(MouseEventKind::Motion, MouseButtonCode::None, 0, 0),
                sgr_modes(MouseTrackingMode::AnyEvent)
            ),
            Some(b"\x1b[<35;1;1M".to_vec())
        );
    }

    #[test]
    fn x10_encoder_suppresses_out_of_range_coordinates() {
        assert_eq!(
            encode_terminal_mouse_event(
                event(MouseEventKind::Press, MouseButtonCode::Left, 222, 222),
                x10_modes(MouseTrackingMode::Normal)
            ),
            Some(vec![0x1b, b'[', b'M', 32, 255, 255])
        );
        assert_eq!(
            encode_terminal_mouse_event(
                event(MouseEventKind::Press, MouseButtonCode::Left, 0, 223),
                x10_modes(MouseTrackingMode::Normal)
            ),
            None
        );
    }

    #[test]
    fn utf8_encoder_extends_legacy_mouse_coordinates() {
        assert_eq!(
            encode_terminal_mouse_event(
                event(MouseEventKind::Press, MouseButtonCode::Left, 0, 0),
                utf8_modes(MouseTrackingMode::Normal)
            ),
            Some(vec![0x1b, b'[', b'M', 32, 33, 33])
        );
        assert_eq!(
            encode_terminal_mouse_event(
                event(MouseEventKind::Press, MouseButtonCode::Left, 0, 223),
                utf8_modes(MouseTrackingMode::Normal)
            ),
            Some(vec![0x1b, b'[', b'M', 32, 0xc4, 0x80, 33])
        );
    }

    #[test]
    fn urxvt_encoder_uses_decimal_legacy_mouse_coordinates() {
        let modes = urxvt_modes(MouseTrackingMode::ButtonEvent);

        assert_eq!(
            encode_terminal_mouse_event(
                event(MouseEventKind::Press, MouseButtonCode::Left, 0, 0),
                modes
            ),
            Some(b"\x1b[32;1;1M".to_vec())
        );
        assert_eq!(
            encode_terminal_mouse_event(
                event(MouseEventKind::Release, MouseButtonCode::Left, 0, 0),
                modes
            ),
            Some(b"\x1b[35;1;1M".to_vec())
        );
        assert_eq!(
            encode_terminal_mouse_event(
                event(MouseEventKind::Motion, MouseButtonCode::Left, 1, 223),
                modes
            ),
            Some(b"\x1b[64;224;2M".to_vec())
        );
        assert_eq!(
            encode_terminal_mouse_event(
                event(MouseEventKind::Wheel, MouseButtonCode::WheelDown, 9, 499),
                modes
            ),
            Some(b"\x1b[97;500;10M".to_vec())
        );
    }

    #[test]
    fn focus_encoder_emits_xterm_focus_events_only_when_enabled() {
        let disabled = TerminalMouseModes::default();
        let enabled = TerminalMouseModes {
            focus_events: true,
            ..TerminalMouseModes::default()
        };

        assert_eq!(
            encode_terminal_focus_event(FocusEventKind::In, disabled),
            None
        );
        assert_eq!(
            encode_terminal_focus_event(FocusEventKind::In, enabled),
            Some(b"\x1b[I".to_vec())
        );
        assert_eq!(
            encode_terminal_focus_event(FocusEventKind::Out, enabled),
            Some(b"\x1b[O".to_vec())
        );
    }
}
