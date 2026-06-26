use crate::{
  TerminalInputModes, KITTY_KEYBOARD_DISAMBIGUATE_ESC_CODES,
  KITTY_KEYBOARD_REPORT_ALL_KEYS_AS_ESC_CODES, KITTY_KEYBOARD_REPORT_ALTERNATE_KEYS,
  KITTY_KEYBOARD_REPORT_ASSOCIATED_TEXT, KITTY_KEYBOARD_REPORT_EVENT_TYPES,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalKey<'a> {
  Named(TerminalNamedKey),
  Character(&'a str),
  Unidentified,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalNamedKey {
  Enter,
  Tab,
  Backspace,
  Escape,
  ArrowUp,
  ArrowDown,
  ArrowRight,
  ArrowLeft,
  Home,
  End,
  Insert,
  PageUp,
  PageDown,
  Delete,
  F1,
  F2,
  F3,
  F4,
  F5,
  F6,
  F7,
  F8,
  F9,
  F10,
  F11,
  F12,
  F13,
  F14,
  F15,
  F16,
  F17,
  F18,
  F19,
  F20,
  F21,
  F22,
  F23,
  F24,
  F25,
  F26,
  F27,
  F28,
  F29,
  F30,
  F31,
  F32,
  F33,
  F34,
  F35,
  CapsLock,
  ScrollLock,
  NumLock,
  PrintScreen,
  Pause,
  ContextMenu,
  MediaPlay,
  MediaPause,
  MediaPlayPause,
  MediaStop,
  MediaFastForward,
  MediaRewind,
  MediaTrackNext,
  MediaTrackPrevious,
  MediaRecord,
  AudioVolumeDown,
  AudioVolumeUp,
  AudioVolumeMute,
  AltGraph,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TerminalKeyEventType {
  #[default]
  Press,
  Repeat,
  Release,
}

impl TerminalKeyEventType {
  pub fn from_browser_event_type(value: u8) -> Self {
    match value {
      2 => Self::Repeat,
      3 => Self::Release,
      _ => Self::Press,
    }
  }

  fn kitty_parameter(self) -> u8 {
    match self {
      Self::Press => 1,
      Self::Repeat => 2,
      Self::Release => 3,
    }
  }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TerminalKeyModifiers {
  pub control: bool,
  pub shift: bool,
  pub alt: bool,
  pub meta: bool,
  pub hyper: bool,
  pub kitty_meta: bool,
}

impl TerminalKeyModifiers {
  pub fn from_browser_mask(control: bool, mask: u8) -> Self {
    Self {
      control,
      shift: mask & 0b001 != 0,
      alt: mask & 0b010 != 0,
      meta: mask & 0b100 != 0,
      hyper: false,
      kitty_meta: false,
    }
  }

  pub fn with_modifier_key_event_state(
    mut self,
    modifier_key: Option<TerminalModifierKey>,
    event_type: TerminalKeyEventType,
  ) -> Self {
    self.apply_modifier_key_event_state(modifier_key, event_type);
    self
  }

  pub fn apply_modifier_key_event_state(
    &mut self,
    modifier_key: Option<TerminalModifierKey>,
    event_type: TerminalKeyEventType,
  ) {
    let Some(modifier_key) = modifier_key else {
      return;
    };
    let active = event_type != TerminalKeyEventType::Release;
    match modifier_key {
      TerminalModifierKey::LeftShift | TerminalModifierKey::RightShift => self.shift = active,
      TerminalModifierKey::LeftAlt | TerminalModifierKey::RightAlt => self.alt = active,
      TerminalModifierKey::LeftControl | TerminalModifierKey::RightControl => self.control = active,
      TerminalModifierKey::LeftSuper | TerminalModifierKey::RightSuper => self.meta = active,
      TerminalModifierKey::LeftHyper | TerminalModifierKey::RightHyper => self.hyper = active,
      TerminalModifierKey::LeftMeta | TerminalModifierKey::RightMeta => self.kitty_meta = active,
    }
  }

  fn allows_application_keypad(self) -> bool {
    !self.control && !self.shift && !self.alt && !self.meta && !self.hyper && !self.kitty_meta
  }

  fn xterm_parameter(self) -> Option<u8> {
    if self.meta || self.hyper || self.kitty_meta {
      return None;
    }

    let mut parameter = 1;
    if self.shift {
      parameter += 1;
    }
    if self.alt {
      parameter += 2;
    }
    if self.control {
      parameter += 4;
    }

    (parameter > 1).then_some(parameter)
  }

  fn kitty_parameter(self) -> u16 {
    let mut bits = 0;
    if self.shift {
      bits |= 1;
    }
    if self.alt {
      bits |= 2;
    }
    if self.control {
      bits |= 4;
    }
    if self.meta {
      bits |= 8;
    }
    if self.hyper {
      bits |= 16;
    }
    if self.kitty_meta {
      bits |= 32;
    }
    bits + 1
  }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalModifierKey {
  LeftShift,
  RightShift,
  LeftAlt,
  RightAlt,
  LeftControl,
  RightControl,
  LeftSuper,
  RightSuper,
  LeftHyper,
  RightHyper,
  LeftMeta,
  RightMeta,
}

impl TerminalModifierKey {
  fn kitty_key_code(self) -> u32 {
    match self {
      Self::LeftShift => 57441,
      Self::LeftControl => 57442,
      Self::LeftAlt => 57443,
      Self::LeftSuper => 57444,
      Self::LeftHyper => 57445,
      Self::LeftMeta => 57446,
      Self::RightShift => 57447,
      Self::RightControl => 57448,
      Self::RightAlt => 57449,
      Self::RightSuper => 57450,
      Self::RightHyper => 57451,
      Self::RightMeta => 57452,
    }
  }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalKeypadKey {
  Digit(u8),
  Decimal,
  Comma,
  Add,
  Subtract,
  Multiply,
  Divide,
  Enter,
  Equal,
  Left,
  Right,
  Up,
  Down,
  PageUp,
  PageDown,
  Home,
  End,
  Insert,
  Delete,
  Begin,
}

impl TerminalKeypadKey {
  fn kitty_key_code(self) -> u32 {
    match self {
      Self::Digit(value) => 57399 + u32::from(value),
      Self::Decimal => 57409,
      Self::Divide => 57410,
      Self::Multiply => 57411,
      Self::Subtract => 57412,
      Self::Add => 57413,
      Self::Enter => 57414,
      Self::Equal => 57415,
      Self::Comma => 57416,
      Self::Left => 57417,
      Self::Right => 57418,
      Self::Up => 57419,
      Self::Down => 57420,
      Self::PageUp => 57421,
      Self::PageDown => 57422,
      Self::Home => 57423,
      Self::End => 57424,
      Self::Insert => 57425,
      Self::Delete => 57426,
      Self::Begin => 57427,
    }
  }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerminalKeyInput<'a> {
  pub key: TerminalKey<'a>,
  pub text: Option<&'a str>,
  pub modifiers: TerminalKeyModifiers,
  pub keypad_key: Option<TerminalKeypadKey>,
  pub base_layout_key: Option<char>,
  pub modifier_key: Option<TerminalModifierKey>,
  pub event_type: TerminalKeyEventType,
}

pub fn encode_terminal_key_input(
  input: TerminalKeyInput<'_>,
  modes: TerminalInputModes,
) -> Option<Vec<u8>> {
  if modes.keyboard_locked {
    return None;
  }

  let report_event_types = kitty_keyboard_report_event_types_enabled(modes);
  let report_alternate_keys = kitty_keyboard_report_alternate_keys_enabled(modes);
  if input.event_type == TerminalKeyEventType::Release && !report_event_types {
    return None;
  }

  if kitty_keyboard_report_all_keys_enabled(modes) {
    if let Some(bytes) = kitty_all_keys_sequence(
      input,
      kitty_keyboard_report_associated_text_enabled(modes),
      report_alternate_keys,
      report_event_types,
    ) {
      return Some(bytes);
    }
  }

  if kitty_keyboard_disambiguate_enabled(modes) {
    if let Some(bytes) =
      kitty_disambiguated_key_sequence(input, report_alternate_keys, report_event_types)
    {
      return Some(bytes);
    }
  }

  if input.event_type == TerminalKeyEventType::Release {
    return None;
  }

  if modes.application_keypad && input.modifiers.allows_application_keypad() {
    if let Some(bytes) = input.keypad_key.and_then(application_keypad_sequence) {
      return Some(bytes);
    }
  }

  if let Some(parameter) = input.modifiers.xterm_parameter() {
    if let Some(bytes) = modified_named_key_sequence(input.key, parameter) {
      return Some(bytes);
    }
  }

  match input.key {
    TerminalKey::Named(TerminalNamedKey::Enter) => Some(b"\r".to_vec()),
    TerminalKey::Named(TerminalNamedKey::Tab) => Some(b"\t".to_vec()),
    TerminalKey::Named(TerminalNamedKey::Backspace) => Some(backspace_sequence(modes)),
    TerminalKey::Named(TerminalNamedKey::Escape) => Some(b"\x1b".to_vec()),
    TerminalKey::Named(TerminalNamedKey::ArrowUp) => Some(cursor_key_sequence(b'A', modes)),
    TerminalKey::Named(TerminalNamedKey::ArrowDown) => Some(cursor_key_sequence(b'B', modes)),
    TerminalKey::Named(TerminalNamedKey::ArrowRight) => Some(cursor_key_sequence(b'C', modes)),
    TerminalKey::Named(TerminalNamedKey::ArrowLeft) => Some(cursor_key_sequence(b'D', modes)),
    TerminalKey::Named(TerminalNamedKey::Home) => Some(cursor_key_sequence(b'H', modes)),
    TerminalKey::Named(TerminalNamedKey::End) => Some(cursor_key_sequence(b'F', modes)),
    TerminalKey::Named(TerminalNamedKey::Insert) => Some(csi_tilde_sequence(2)),
    TerminalKey::Named(TerminalNamedKey::PageUp) => Some(b"\x1b[5~".to_vec()),
    TerminalKey::Named(TerminalNamedKey::PageDown) => Some(b"\x1b[6~".to_vec()),
    TerminalKey::Named(TerminalNamedKey::Delete) => Some(b"\x1b[3~".to_vec()),
    TerminalKey::Named(TerminalNamedKey::F1) => Some(ss3_sequence(b'P')),
    TerminalKey::Named(TerminalNamedKey::F2) => Some(ss3_sequence(b'Q')),
    TerminalKey::Named(TerminalNamedKey::F3) => Some(ss3_sequence(b'R')),
    TerminalKey::Named(TerminalNamedKey::F4) => Some(ss3_sequence(b'S')),
    TerminalKey::Named(TerminalNamedKey::F5) => Some(csi_tilde_sequence(15)),
    TerminalKey::Named(TerminalNamedKey::F6) => Some(csi_tilde_sequence(17)),
    TerminalKey::Named(TerminalNamedKey::F7) => Some(csi_tilde_sequence(18)),
    TerminalKey::Named(TerminalNamedKey::F8) => Some(csi_tilde_sequence(19)),
    TerminalKey::Named(TerminalNamedKey::F9) => Some(csi_tilde_sequence(20)),
    TerminalKey::Named(TerminalNamedKey::F10) => Some(csi_tilde_sequence(21)),
    TerminalKey::Named(TerminalNamedKey::F11) => Some(csi_tilde_sequence(23)),
    TerminalKey::Named(TerminalNamedKey::F12) => Some(csi_tilde_sequence(24)),
    TerminalKey::Character(value) if input.modifiers.control => encode_control_character(value),
    _ => input.text.and_then(non_empty_bytes),
  }
}

fn kitty_keyboard_disambiguate_enabled(modes: TerminalInputModes) -> bool {
  modes.kitty_keyboard_flags & KITTY_KEYBOARD_DISAMBIGUATE_ESC_CODES != 0
}

fn kitty_keyboard_report_all_keys_enabled(modes: TerminalInputModes) -> bool {
  modes.kitty_keyboard_flags & KITTY_KEYBOARD_REPORT_ALL_KEYS_AS_ESC_CODES != 0
}

fn kitty_keyboard_report_associated_text_enabled(modes: TerminalInputModes) -> bool {
  modes.kitty_keyboard_flags & KITTY_KEYBOARD_REPORT_ASSOCIATED_TEXT != 0
}

fn kitty_keyboard_report_event_types_enabled(modes: TerminalInputModes) -> bool {
  modes.kitty_keyboard_flags & KITTY_KEYBOARD_REPORT_EVENT_TYPES != 0
}

fn kitty_keyboard_report_alternate_keys_enabled(modes: TerminalInputModes) -> bool {
  modes.kitty_keyboard_flags & KITTY_KEYBOARD_REPORT_ALTERNATE_KEYS != 0
}

fn kitty_all_keys_sequence(
  input: TerminalKeyInput<'_>,
  report_associated_text: bool,
  report_alternate_keys: bool,
  report_event_type: bool,
) -> Option<Vec<u8>> {
  if let Some(modifier_key) = input.modifier_key {
    return Some(kitty_csi_u_sequence(
      KittyKeyCodes::new(modifier_key.kitty_key_code()),
      input.modifiers,
      input.event_type,
      report_event_type,
    ));
  }
  if let Some(bytes) = kitty_keypad_sequence(input, report_associated_text, report_event_type) {
    return Some(bytes);
  }
  if let Some(bytes) = kitty_functional_key_sequence(input, report_event_type) {
    return Some(bytes);
  }

  match input.key {
    TerminalKey::Named(TerminalNamedKey::Escape) => Some(kitty_csi_u_sequence_with_text(
      KittyKeyCodes::new(27),
      input.modifiers,
      input.event_type,
      report_event_type,
      kitty_associated_text(input, report_associated_text, false),
    )),
    TerminalKey::Named(TerminalNamedKey::Enter) => Some(kitty_csi_u_sequence_with_text(
      KittyKeyCodes::new(13),
      input.modifiers,
      input.event_type,
      report_event_type,
      kitty_associated_text(input, report_associated_text, false),
    )),
    TerminalKey::Named(TerminalNamedKey::Tab) => Some(kitty_csi_u_sequence_with_text(
      KittyKeyCodes::new(9),
      input.modifiers,
      input.event_type,
      report_event_type,
      kitty_associated_text(input, report_associated_text, false),
    )),
    TerminalKey::Named(TerminalNamedKey::Backspace) => Some(kitty_csi_u_sequence_with_text(
      KittyKeyCodes::new(127),
      input.modifiers,
      input.event_type,
      report_event_type,
      kitty_associated_text(input, report_associated_text, false),
    )),
    TerminalKey::Character(value) => {
      let text = kitty_associated_text(input, report_associated_text, true);
      let key_codes = kitty_character_key_codes(value, input, report_alternate_keys)
        .unwrap_or_else(|| KittyKeyCodes::new(0));
      Some(kitty_csi_u_sequence_with_text(
        key_codes,
        input.modifiers,
        input.event_type,
        report_event_type,
        text,
      ))
    }
    TerminalKey::Unidentified => {
      let text = kitty_associated_text(input, report_associated_text, true);
      input.text.filter(|text| !text.is_empty()).map(|_| {
        kitty_csi_u_sequence_with_text(
          KittyKeyCodes::new(0),
          input.modifiers,
          input.event_type,
          report_event_type,
          text,
        )
      })
    }
    _ => None,
  }
}

fn kitty_disambiguated_key_sequence(
  input: TerminalKeyInput<'_>,
  report_alternate_keys: bool,
  report_event_type: bool,
) -> Option<Vec<u8>> {
  if let Some(bytes) = kitty_keypad_sequence(input, false, report_event_type) {
    return Some(bytes);
  }
  if let Some(bytes) = kitty_functional_key_sequence(input, report_event_type) {
    return Some(bytes);
  }

  match input.key {
    TerminalKey::Named(TerminalNamedKey::Escape) => Some(kitty_csi_u_sequence(
      KittyKeyCodes::new(27),
      input.modifiers,
      input.event_type,
      report_event_type,
    )),
    TerminalKey::Character(value)
      if input.modifiers.control || input.modifiers.alt || input.modifiers.meta =>
    {
      kitty_character_key_codes(value, input, report_alternate_keys).map(|key_codes| {
        kitty_csi_u_sequence(
          key_codes,
          input.modifiers,
          input.event_type,
          report_event_type,
        )
      })
    }
    _ => None,
  }
}

fn kitty_keypad_sequence(
  input: TerminalKeyInput<'_>,
  report_associated_text: bool,
  report_event_type: bool,
) -> Option<Vec<u8>> {
  let keypad_key = input.keypad_key?;
  Some(kitty_csi_u_sequence_with_text(
    KittyKeyCodes::new(keypad_key.kitty_key_code()),
    input.modifiers,
    input.event_type,
    report_event_type,
    kitty_associated_text(input, report_associated_text, true),
  ))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum KittyFunctionalKey {
  Final { base_parameter: u8, final_byte: u8 },
  Tilde(u8),
  CsiU(u32),
}

impl KittyFunctionalKey {
  fn sequence(
    self,
    modifiers: TerminalKeyModifiers,
    event_type: TerminalKeyEventType,
    report_event_type: bool,
  ) -> Vec<u8> {
    match self {
      Self::Final {
        base_parameter,
        final_byte,
      } => kitty_functional_final_sequence(
        base_parameter,
        final_byte,
        modifiers,
        event_type,
        report_event_type,
      ),
      Self::Tilde(base_parameter) => {
        kitty_functional_tilde_sequence(base_parameter, modifiers, event_type, report_event_type)
      }
      Self::CsiU(code) => kitty_csi_u_sequence(
        KittyKeyCodes::new(code),
        modifiers,
        event_type,
        report_event_type,
      ),
    }
  }

  fn has_legacy_fallback(self) -> bool {
    !matches!(self, Self::CsiU(_))
  }
}

fn kitty_functional_key_sequence(
  input: TerminalKeyInput<'_>,
  report_event_type: bool,
) -> Option<Vec<u8>> {
  let key = kitty_functional_key(input.key)?;
  if key.has_legacy_fallback() && !report_event_type && !input.modifiers.meta {
    return None;
  }

  Some(key.sequence(input.modifiers, input.event_type, report_event_type))
}

fn kitty_functional_key(key: TerminalKey<'_>) -> Option<KittyFunctionalKey> {
  match key {
    TerminalKey::Named(TerminalNamedKey::ArrowUp) => Some(KittyFunctionalKey::Final {
      base_parameter: 1,
      final_byte: b'A',
    }),
    TerminalKey::Named(TerminalNamedKey::ArrowDown) => Some(KittyFunctionalKey::Final {
      base_parameter: 1,
      final_byte: b'B',
    }),
    TerminalKey::Named(TerminalNamedKey::ArrowRight) => Some(KittyFunctionalKey::Final {
      base_parameter: 1,
      final_byte: b'C',
    }),
    TerminalKey::Named(TerminalNamedKey::ArrowLeft) => Some(KittyFunctionalKey::Final {
      base_parameter: 1,
      final_byte: b'D',
    }),
    TerminalKey::Named(TerminalNamedKey::Home) => Some(KittyFunctionalKey::Final {
      base_parameter: 1,
      final_byte: b'H',
    }),
    TerminalKey::Named(TerminalNamedKey::End) => Some(KittyFunctionalKey::Final {
      base_parameter: 1,
      final_byte: b'F',
    }),
    TerminalKey::Named(TerminalNamedKey::Insert) => Some(KittyFunctionalKey::Tilde(2)),
    TerminalKey::Named(TerminalNamedKey::Delete) => Some(KittyFunctionalKey::Tilde(3)),
    TerminalKey::Named(TerminalNamedKey::PageUp) => Some(KittyFunctionalKey::Tilde(5)),
    TerminalKey::Named(TerminalNamedKey::PageDown) => Some(KittyFunctionalKey::Tilde(6)),
    TerminalKey::Named(TerminalNamedKey::F1) => Some(KittyFunctionalKey::Final {
      base_parameter: 1,
      final_byte: b'P',
    }),
    TerminalKey::Named(TerminalNamedKey::F2) => Some(KittyFunctionalKey::Final {
      base_parameter: 1,
      final_byte: b'Q',
    }),
    TerminalKey::Named(TerminalNamedKey::F3) => Some(KittyFunctionalKey::Tilde(13)),
    TerminalKey::Named(TerminalNamedKey::F4) => Some(KittyFunctionalKey::Final {
      base_parameter: 1,
      final_byte: b'S',
    }),
    TerminalKey::Named(TerminalNamedKey::F5) => Some(KittyFunctionalKey::Tilde(15)),
    TerminalKey::Named(TerminalNamedKey::F6) => Some(KittyFunctionalKey::Tilde(17)),
    TerminalKey::Named(TerminalNamedKey::F7) => Some(KittyFunctionalKey::Tilde(18)),
    TerminalKey::Named(TerminalNamedKey::F8) => Some(KittyFunctionalKey::Tilde(19)),
    TerminalKey::Named(TerminalNamedKey::F9) => Some(KittyFunctionalKey::Tilde(20)),
    TerminalKey::Named(TerminalNamedKey::F10) => Some(KittyFunctionalKey::Tilde(21)),
    TerminalKey::Named(TerminalNamedKey::F11) => Some(KittyFunctionalKey::Tilde(23)),
    TerminalKey::Named(TerminalNamedKey::F12) => Some(KittyFunctionalKey::Tilde(24)),
    TerminalKey::Named(named_key) => {
      kitty_pua_functional_key_code(named_key).map(KittyFunctionalKey::CsiU)
    }
    _ => None,
  }
}

fn kitty_pua_functional_key_code(named_key: TerminalNamedKey) -> Option<u32> {
  match named_key {
    TerminalNamedKey::CapsLock => Some(57358),
    TerminalNamedKey::ScrollLock => Some(57359),
    TerminalNamedKey::NumLock => Some(57360),
    TerminalNamedKey::PrintScreen => Some(57361),
    TerminalNamedKey::Pause => Some(57362),
    TerminalNamedKey::ContextMenu => Some(57363),
    TerminalNamedKey::F13 => Some(57376),
    TerminalNamedKey::F14 => Some(57377),
    TerminalNamedKey::F15 => Some(57378),
    TerminalNamedKey::F16 => Some(57379),
    TerminalNamedKey::F17 => Some(57380),
    TerminalNamedKey::F18 => Some(57381),
    TerminalNamedKey::F19 => Some(57382),
    TerminalNamedKey::F20 => Some(57383),
    TerminalNamedKey::F21 => Some(57384),
    TerminalNamedKey::F22 => Some(57385),
    TerminalNamedKey::F23 => Some(57386),
    TerminalNamedKey::F24 => Some(57387),
    TerminalNamedKey::F25 => Some(57388),
    TerminalNamedKey::F26 => Some(57389),
    TerminalNamedKey::F27 => Some(57390),
    TerminalNamedKey::F28 => Some(57391),
    TerminalNamedKey::F29 => Some(57392),
    TerminalNamedKey::F30 => Some(57393),
    TerminalNamedKey::F31 => Some(57394),
    TerminalNamedKey::F32 => Some(57395),
    TerminalNamedKey::F33 => Some(57396),
    TerminalNamedKey::F34 => Some(57397),
    TerminalNamedKey::F35 => Some(57398),
    TerminalNamedKey::MediaPlay => Some(57428),
    TerminalNamedKey::MediaPause => Some(57429),
    TerminalNamedKey::MediaPlayPause => Some(57430),
    TerminalNamedKey::MediaStop => Some(57432),
    TerminalNamedKey::MediaFastForward => Some(57433),
    TerminalNamedKey::MediaRewind => Some(57434),
    TerminalNamedKey::MediaTrackNext => Some(57435),
    TerminalNamedKey::MediaTrackPrevious => Some(57436),
    TerminalNamedKey::MediaRecord => Some(57437),
    TerminalNamedKey::AudioVolumeDown => Some(57438),
    TerminalNamedKey::AudioVolumeUp => Some(57439),
    TerminalNamedKey::AudioVolumeMute => Some(57440),
    TerminalNamedKey::AltGraph => Some(57453),
    _ => None,
  }
}

fn kitty_functional_modifier_field(
  modifiers: TerminalKeyModifiers,
  event_type: TerminalKeyEventType,
  report_event_type: bool,
) -> Option<String> {
  let modifier_parameter = modifiers.kitty_parameter();
  if report_event_type {
    Some(format!(
      "{modifier_parameter}:{}",
      event_type.kitty_parameter()
    ))
  } else if modifier_parameter != 1 {
    Some(modifier_parameter.to_string())
  } else {
    None
  }
}

fn kitty_functional_final_sequence(
  base_parameter: u8,
  final_byte: u8,
  modifiers: TerminalKeyModifiers,
  event_type: TerminalKeyEventType,
  report_event_type: bool,
) -> Vec<u8> {
  let mut bytes = if let Some(modifier) =
    kitty_functional_modifier_field(modifiers, event_type, report_event_type)
  {
    format!("\x1b[{base_parameter};{modifier}").into_bytes()
  } else if base_parameter == 1 {
    b"\x1b[".to_vec()
  } else {
    format!("\x1b[{base_parameter}").into_bytes()
  };
  bytes.push(final_byte);
  bytes
}

fn kitty_functional_tilde_sequence(
  base_parameter: u8,
  modifiers: TerminalKeyModifiers,
  event_type: TerminalKeyEventType,
  report_event_type: bool,
) -> Vec<u8> {
  if let Some(modifier) = kitty_functional_modifier_field(modifiers, event_type, report_event_type)
  {
    format!("\x1b[{base_parameter};{modifier}~").into_bytes()
  } else {
    format!("\x1b[{base_parameter}~").into_bytes()
  }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct KittyKeyCodes {
  primary: u32,
  shifted: Option<u32>,
  base: Option<u32>,
}

impl KittyKeyCodes {
  fn new(primary: u32) -> Self {
    Self {
      primary,
      shifted: None,
      base: None,
    }
  }

  fn parameter(self) -> String {
    match (self.shifted, self.base) {
      (Some(shifted), Some(base)) => format!("{}:{shifted}:{base}", self.primary),
      (Some(shifted), None) => format!("{}:{shifted}", self.primary),
      (None, Some(base)) => format!("{}::{base}", self.primary),
      (None, None) => self.primary.to_string(),
    }
  }
}

fn kitty_character_key_codes(
  value: &str,
  input: TerminalKeyInput<'_>,
  report_alternate_keys: bool,
) -> Option<KittyKeyCodes> {
  let primary = kitty_primary_character_key_code(value, input.base_layout_key)?;
  if !report_alternate_keys {
    return Some(KittyKeyCodes::new(primary));
  }

  let shifted = kitty_shifted_character_key_code(value, input, primary);
  let base = kitty_base_layout_key_code(input.base_layout_key, primary, shifted);
  Some(KittyKeyCodes {
    primary,
    shifted,
    base,
  })
}

fn kitty_primary_character_key_code(value: &str, base_layout_key: Option<char>) -> Option<u32> {
  let mut chars = value.chars();
  let ch = chars.next()?;
  if chars.next().is_some() {
    return None;
  }
  if let Some(base) = base_layout_key {
    if shifted_us_layout_key(base) == Some(ch) {
      return Some(base as u32);
    }
  }
  Some(kitty_lowercase_character(ch) as u32)
}

fn kitty_shifted_character_key_code(
  value: &str,
  input: TerminalKeyInput<'_>,
  primary: u32,
) -> Option<u32> {
  if !input.modifiers.shift {
    return None;
  }
  let ch = input
    .text
    .and_then(single_character)
    .or_else(|| single_character(value))?;
  let code = ch as u32;
  (code != primary).then_some(code)
}

fn kitty_base_layout_key_code(
  base_layout_key: Option<char>,
  primary: u32,
  shifted: Option<u32>,
) -> Option<u32> {
  let code = base_layout_key? as u32;
  if code == primary || Some(code) == shifted {
    None
  } else {
    Some(code)
  }
}

fn single_character(value: &str) -> Option<char> {
  let mut chars = value.chars();
  let ch = chars.next()?;
  chars.next().is_none().then_some(ch)
}

fn kitty_lowercase_character(ch: char) -> char {
  if ch.is_ascii_alphabetic() {
    return ch.to_ascii_lowercase();
  }

  let mut lower = ch.to_lowercase();
  let Some(first) = lower.next() else {
    return ch;
  };
  if lower.next().is_none() {
    first
  } else {
    ch
  }
}

fn kitty_csi_u_sequence(
  key_codes: KittyKeyCodes,
  modifiers: TerminalKeyModifiers,
  event_type: TerminalKeyEventType,
  report_event_type: bool,
) -> Vec<u8> {
  kitty_csi_u_sequence_with_text(key_codes, modifiers, event_type, report_event_type, None)
}

fn kitty_csi_u_sequence_with_text(
  key_codes: KittyKeyCodes,
  modifiers: TerminalKeyModifiers,
  event_type: TerminalKeyEventType,
  report_event_type: bool,
  associated_text: Option<String>,
) -> Vec<u8> {
  let modifier_parameter = modifiers.kitty_parameter();
  let modifier_field = if report_event_type {
    Some(format!(
      "{modifier_parameter}:{}",
      event_type.kitty_parameter()
    ))
  } else if modifier_parameter != 1 {
    Some(modifier_parameter.to_string())
  } else {
    None
  };
  let key_code = key_codes.parameter();

  match (modifier_field, associated_text) {
    (Some(modifier), Some(text)) => format!("\x1b[{key_code};{modifier};{text}u"),
    (Some(modifier), None) => format!("\x1b[{key_code};{modifier}u"),
    (None, Some(text)) => format!("\x1b[{key_code};;{text}u"),
    (None, None) => format!("\x1b[{key_code}u"),
  }
  .into_bytes()
}

fn kitty_associated_text(
  input: TerminalKeyInput<'_>,
  report_associated_text: bool,
  text_key: bool,
) -> Option<String> {
  if !report_associated_text || !text_key || input.modifiers.control || input.modifiers.meta {
    return None;
  }
  let text = input.text?;
  if text.is_empty() {
    return None;
  }
  kitty_associated_text_parameter(text)
}

fn kitty_associated_text_parameter(text: &str) -> Option<String> {
  let mut codes = Vec::new();
  for ch in text.chars() {
    let code = ch as u32;
    if code <= 0x1f || code == 0x7f || (0x80..=0x9f).contains(&code) {
      return None;
    }
    codes.push(code.to_string());
  }
  (!codes.is_empty()).then(|| codes.join(":"))
}

fn backspace_sequence(modes: TerminalInputModes) -> Vec<u8> {
  if modes.backarrow_sends_backspace {
    b"\x08".to_vec()
  } else {
    b"\x7f".to_vec()
  }
}

fn modified_named_key_sequence(key: TerminalKey<'_>, modifier_parameter: u8) -> Option<Vec<u8>> {
  match key {
    TerminalKey::Named(TerminalNamedKey::ArrowUp) => {
      Some(csi_modified_final_sequence(1, modifier_parameter, b'A'))
    }
    TerminalKey::Named(TerminalNamedKey::ArrowDown) => {
      Some(csi_modified_final_sequence(1, modifier_parameter, b'B'))
    }
    TerminalKey::Named(TerminalNamedKey::ArrowRight) => {
      Some(csi_modified_final_sequence(1, modifier_parameter, b'C'))
    }
    TerminalKey::Named(TerminalNamedKey::ArrowLeft) => {
      Some(csi_modified_final_sequence(1, modifier_parameter, b'D'))
    }
    TerminalKey::Named(TerminalNamedKey::Home) => {
      Some(csi_modified_final_sequence(1, modifier_parameter, b'H'))
    }
    TerminalKey::Named(TerminalNamedKey::End) => {
      Some(csi_modified_final_sequence(1, modifier_parameter, b'F'))
    }
    TerminalKey::Named(TerminalNamedKey::Insert) => {
      Some(csi_modified_tilde_sequence(2, modifier_parameter))
    }
    TerminalKey::Named(TerminalNamedKey::Delete) => {
      Some(csi_modified_tilde_sequence(3, modifier_parameter))
    }
    TerminalKey::Named(TerminalNamedKey::PageUp) => {
      Some(csi_modified_tilde_sequence(5, modifier_parameter))
    }
    TerminalKey::Named(TerminalNamedKey::PageDown) => {
      Some(csi_modified_tilde_sequence(6, modifier_parameter))
    }
    TerminalKey::Named(TerminalNamedKey::F1) => {
      Some(csi_modified_final_sequence(1, modifier_parameter, b'P'))
    }
    TerminalKey::Named(TerminalNamedKey::F2) => {
      Some(csi_modified_final_sequence(1, modifier_parameter, b'Q'))
    }
    TerminalKey::Named(TerminalNamedKey::F3) => {
      Some(csi_modified_final_sequence(1, modifier_parameter, b'R'))
    }
    TerminalKey::Named(TerminalNamedKey::F4) => {
      Some(csi_modified_final_sequence(1, modifier_parameter, b'S'))
    }
    TerminalKey::Named(TerminalNamedKey::F5) => {
      Some(csi_modified_tilde_sequence(15, modifier_parameter))
    }
    TerminalKey::Named(TerminalNamedKey::F6) => {
      Some(csi_modified_tilde_sequence(17, modifier_parameter))
    }
    TerminalKey::Named(TerminalNamedKey::F7) => {
      Some(csi_modified_tilde_sequence(18, modifier_parameter))
    }
    TerminalKey::Named(TerminalNamedKey::F8) => {
      Some(csi_modified_tilde_sequence(19, modifier_parameter))
    }
    TerminalKey::Named(TerminalNamedKey::F9) => {
      Some(csi_modified_tilde_sequence(20, modifier_parameter))
    }
    TerminalKey::Named(TerminalNamedKey::F10) => {
      Some(csi_modified_tilde_sequence(21, modifier_parameter))
    }
    TerminalKey::Named(TerminalNamedKey::F11) => {
      Some(csi_modified_tilde_sequence(23, modifier_parameter))
    }
    TerminalKey::Named(TerminalNamedKey::F12) => {
      Some(csi_modified_tilde_sequence(24, modifier_parameter))
    }
    _ => None,
  }
}

pub fn shifted_us_layout_key(base: char) -> Option<char> {
  match base {
    '`' => Some('~'),
    '1' => Some('!'),
    '2' => Some('@'),
    '3' => Some('#'),
    '4' => Some('$'),
    '5' => Some('%'),
    '6' => Some('^'),
    '7' => Some('&'),
    '8' => Some('*'),
    '9' => Some('('),
    '0' => Some(')'),
    '-' => Some('_'),
    '=' => Some('+'),
    '[' => Some('{'),
    ']' => Some('}'),
    '\\' => Some('|'),
    ';' => Some(':'),
    '\'' => Some('"'),
    ',' => Some('<'),
    '.' => Some('>'),
    '/' => Some('?'),
    _ => None,
  }
}

fn application_keypad_sequence(keypad_key: TerminalKeypadKey) -> Option<Vec<u8>> {
  let final_byte = match keypad_key {
    TerminalKeypadKey::Digit(0) => b'p',
    TerminalKeypadKey::Digit(1) => b'q',
    TerminalKeypadKey::Digit(2) => b'r',
    TerminalKeypadKey::Digit(3) => b's',
    TerminalKeypadKey::Digit(4) => b't',
    TerminalKeypadKey::Digit(5) => b'u',
    TerminalKeypadKey::Digit(6) => b'v',
    TerminalKeypadKey::Digit(7) => b'w',
    TerminalKeypadKey::Digit(8) => b'x',
    TerminalKeypadKey::Digit(9) => b'y',
    TerminalKeypadKey::Multiply => b'j',
    TerminalKeypadKey::Add => b'k',
    TerminalKeypadKey::Comma => b'l',
    TerminalKeypadKey::Subtract => b'm',
    TerminalKeypadKey::Decimal => b'n',
    TerminalKeypadKey::Divide => b'o',
    TerminalKeypadKey::Enter => b'M',
    TerminalKeypadKey::Equal
    | TerminalKeypadKey::Left
    | TerminalKeypadKey::Right
    | TerminalKeypadKey::Up
    | TerminalKeypadKey::Down
    | TerminalKeypadKey::PageUp
    | TerminalKeypadKey::PageDown
    | TerminalKeypadKey::Home
    | TerminalKeypadKey::End
    | TerminalKeypadKey::Insert
    | TerminalKeypadKey::Delete
    | TerminalKeypadKey::Begin
    | TerminalKeypadKey::Digit(_) => return None,
  };
  Some(ss3_sequence(final_byte))
}

fn cursor_key_sequence(final_byte: u8, modes: TerminalInputModes) -> Vec<u8> {
  let prefix = if modes.application_cursor_keys {
    b"\x1bO"
  } else {
    b"\x1b["
  };
  let mut bytes = prefix.to_vec();
  bytes.push(final_byte);
  bytes
}

fn ss3_sequence(final_byte: u8) -> Vec<u8> {
  vec![0x1b, b'O', final_byte]
}

fn csi_tilde_sequence(parameter: u8) -> Vec<u8> {
  format!("\x1b[{parameter}~").into_bytes()
}

fn csi_modified_final_sequence(
  base_parameter: u8,
  modifier_parameter: u8,
  final_byte: u8,
) -> Vec<u8> {
  let mut bytes = format!("\x1b[{base_parameter};{modifier_parameter}").into_bytes();
  bytes.push(final_byte);
  bytes
}

fn csi_modified_tilde_sequence(base_parameter: u8, modifier_parameter: u8) -> Vec<u8> {
  format!("\x1b[{base_parameter};{modifier_parameter}~").into_bytes()
}

fn encode_control_character(value: &str) -> Option<Vec<u8>> {
  let ch = value.chars().next()?.to_ascii_lowercase();
  match ch {
    'a'..='z' => Some(vec![(ch as u8) - b'a' + 1]),
    '[' => Some(vec![0x1b]),
    '\\' => Some(vec![0x1c]),
    ']' => Some(vec![0x1d]),
    '^' => Some(vec![0x1e]),
    '_' => Some(vec![0x1f]),
    '?' => Some(vec![0x7f]),
    _ => None,
  }
}

fn non_empty_bytes(text: &str) -> Option<Vec<u8>> {
  (!text.is_empty()).then(|| text.as_bytes().to_vec())
}

#[cfg(test)]
mod tests {
  use super::*;

  fn text_input<'a>(
    key: TerminalKey<'a>,
    text: Option<&'a str>,
    modifiers: TerminalKeyModifiers,
    modes: TerminalInputModes,
  ) -> Option<Vec<u8>> {
    encode_terminal_key_input(
      TerminalKeyInput {
        key,
        text,
        modifiers,
        keypad_key: None,
        base_layout_key: None,
        modifier_key: None,
        event_type: TerminalKeyEventType::Press,
      },
      modes,
    )
  }

  #[test]
  fn keyboard_encodes_legacy_text_navigation_and_control() {
    assert_eq!(
      text_input(
        TerminalKey::Character("x"),
        Some("x"),
        TerminalKeyModifiers::default(),
        TerminalInputModes::default(),
      ),
      Some(b"x".to_vec())
    );
    assert_eq!(
      text_input(
        TerminalKey::Named(TerminalNamedKey::ArrowUp),
        None,
        TerminalKeyModifiers::default(),
        TerminalInputModes::default(),
      ),
      Some(b"\x1b[A".to_vec())
    );
    assert_eq!(
      text_input(
        TerminalKey::Character("c"),
        None,
        TerminalKeyModifiers {
          control: true,
          ..TerminalKeyModifiers::default()
        },
        TerminalInputModes::default(),
      ),
      Some(vec![0x03])
    );
  }

  #[test]
  fn keyboard_reports_kitty_alternate_text_and_event_type() {
    let modes = TerminalInputModes {
      kitty_keyboard_flags: KITTY_KEYBOARD_REPORT_ALL_KEYS_AS_ESC_CODES
        | KITTY_KEYBOARD_REPORT_ASSOCIATED_TEXT
        | KITTY_KEYBOARD_REPORT_ALTERNATE_KEYS
        | KITTY_KEYBOARD_REPORT_EVENT_TYPES,
      ..TerminalInputModes::default()
    };
    let bytes = encode_terminal_key_input(
      TerminalKeyInput {
        key: TerminalKey::Character("A"),
        text: Some("A"),
        modifiers: TerminalKeyModifiers {
          shift: true,
          ..TerminalKeyModifiers::default()
        },
        keypad_key: None,
        base_layout_key: Some('a'),
        modifier_key: None,
        event_type: TerminalKeyEventType::Repeat,
      },
      modes,
    );

    assert_eq!(bytes, Some(b"\x1b[97:65;2:2;65u".to_vec()));
  }

  #[test]
  fn keyboard_reports_modifier_and_keypad_keys() {
    let modes = TerminalInputModes {
      kitty_keyboard_flags: KITTY_KEYBOARD_REPORT_ALL_KEYS_AS_ESC_CODES,
      ..TerminalInputModes::default()
    };
    assert_eq!(
      encode_terminal_key_input(
        TerminalKeyInput {
          key: TerminalKey::Named(TerminalNamedKey::AltGraph),
          text: None,
          modifiers: TerminalKeyModifiers::default(),
          keypad_key: None,
          base_layout_key: None,
          modifier_key: Some(TerminalModifierKey::RightAlt),
          event_type: TerminalKeyEventType::Press,
        },
        modes,
      ),
      Some(b"\x1b[57449u".to_vec())
    );
    assert_eq!(
      encode_terminal_key_input(
        TerminalKeyInput {
          key: TerminalKey::Character("1"),
          text: Some("1"),
          modifiers: TerminalKeyModifiers::default(),
          keypad_key: Some(TerminalKeypadKey::Digit(1)),
          base_layout_key: None,
          modifier_key: None,
          event_type: TerminalKeyEventType::Press,
        },
        modes,
      ),
      Some(b"\x1b[57400u".to_vec())
    );
  }
}
