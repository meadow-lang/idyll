{-# LANGUAGE PatternSynonyms #-}

-- | Key constants (@keys.rs@).
module System.Console.Rustyline.Keys
  ( KeyEvent (..),
    KeyCode (..),
    Modifiers (..),
    pattern NONE,
    pattern CTRL,
    pattern ALT,
    pattern SHIFT,
    pattern CTRL_SHIFT,
    pattern ALT_SHIFT,
    pattern CTRL_ALT,
    pattern CTRL_ALT_SHIFT,
    hasMods,
    removeMods,
    xorMods,
    newKey,
    ctrl,
    alt,
    fromChar,
    normalize,
    pattern BACKSPACE,
    pattern ENTER,
    pattern ESC,
  )
where

import Data.Bits (complement, xor, (.&.), (.|.))
import Data.Char (isAsciiLower, isAsciiUpper, isControl, toUpper)
import Data.Word (Word8)

-- | Input key pressed and modifiers.
data KeyEvent = KeyEvent KeyCode Modifiers
  deriving (Eq, Ord, Show)

-- | Input key pressed.
data KeyCode
  = -- | Unsupported escape sequence (on unix platform)
    KUnknownEscSeq
  | -- | ⌫ or Ctrl-H
    KBackspace
  | -- | ⇤ (usually Shift-Tab)
    KBackTab
  | -- | Paste (on unix platform)
    KBracketedPasteStart
  | -- | Paste (on unix platform)
    KBracketedPasteEnd
  | -- | Single char
    KChar Char
  | -- | ⌦
    KDelete
  | -- | ↓ arrow key
    KDown
  | -- | ⇲
    KEnd
  | -- | ↵ or Ctrl-M
    KEnter
  | -- | Escape or Ctrl-[
    KEsc
  | -- | Function key
    KF Word8
  | -- | ⇱
    KHome
  | -- | Insert key
    KInsert
  | -- | ← arrow key
    KLeft
  | -- | \\0
    KNull
  | -- | ⇟
    KPageDown
  | -- | ⇞
    KPageUp
  | -- | → arrow key
    KRight
  | -- | ⇥ or Ctrl-I
    KTab
  | -- | ↑ arrow key
    KUp
  deriving (Eq, Ord, Show)

-- | The set of modifier keys that were triggered along with a key press.
newtype Modifiers = Modifiers Word8
  deriving (Eq, Ord)

instance Show Modifiers where
  show m = case m of
    NONE -> "NONE"
    CTRL -> "CTRL"
    ALT -> "ALT"
    SHIFT -> "SHIFT"
    CTRL_SHIFT -> "CTRL_SHIFT"
    ALT_SHIFT -> "ALT_SHIFT"
    CTRL_ALT -> "CTRL_ALT"
    CTRL_ALT_SHIFT -> "CTRL_ALT_SHIFT"
    Modifiers b -> "Modifiers " <> show b

pattern NONE, CTRL, ALT, SHIFT, CTRL_SHIFT, ALT_SHIFT, CTRL_ALT, CTRL_ALT_SHIFT :: Modifiers
-- | No modifier
pattern NONE = Modifiers 0
-- | Control modifier
pattern CTRL = Modifiers 8
-- | Escape or Alt modifier
pattern ALT = Modifiers 4
-- | Shift modifier
pattern SHIFT = Modifiers 2
-- | Ctrl + Shift
pattern CTRL_SHIFT = Modifiers 10
-- | Alt + Shift
pattern ALT_SHIFT = Modifiers 6
-- | Ctrl + Alt
pattern CTRL_ALT = Modifiers 12
-- | Ctrl + Alt + Shift
pattern CTRL_ALT_SHIFT = Modifiers 14

instance Semigroup Modifiers where
  Modifiers a <> Modifiers b = Modifiers (a .|. b)

instance Monoid Modifiers where
  mempty = NONE

-- | @Modifiers::contains@
hasMods :: Modifiers -> Modifiers -> Bool
hasMods (Modifiers m) (Modifiers flag) = m .&. flag == flag

-- | @Modifiers::remove@
removeMods :: Modifiers -> Modifiers -> Modifiers
removeMods (Modifiers m) (Modifiers flag) = Modifiers (m .&. complement flag)

-- | @Modifiers ^ Modifiers@
xorMods :: Modifiers -> Modifiers -> Modifiers
xorMods (Modifiers a) (Modifiers b) = Modifiers (a `xor` b)

pattern BACKSPACE, ENTER, ESC :: KeyEvent
-- | An unmodified press of 'KBackspace'.
pattern BACKSPACE = KeyEvent KBackspace NONE
-- | An unmodified press of 'KEnter'.
pattern ENTER = KeyEvent KEnter NONE
-- | An unmodified press of 'KEsc'.
pattern ESC = KeyEvent KEsc NONE

-- | Constructor from a character and modifiers (@KeyEvent::new@, unix
--   flavour).
newKey :: Char -> Modifiers -> KeyEvent
newKey c mods
  | not (isControl c) =
      let mods' = if mods /= NONE then removeMods mods SHIFT else mods
       in KeyEvent (KChar c) mods'
  | otherwise = case c of
      '\x00' -> KeyEvent (KChar '@') (mods <> CTRL)
      '\x08' -> KeyEvent KBackspace mods
      '\x09'
        | hasMods mods SHIFT -> KeyEvent KBackTab (removeMods mods SHIFT)
        | otherwise -> KeyEvent KTab mods
      '\x0a' -> KeyEvent (KChar 'J') (mods <> CTRL)
      '\x0d' -> KeyEvent KEnter mods
      '\x1b' -> KeyEvent KEsc mods
      '\x1c' -> KeyEvent (KChar '\\') (mods <> CTRL)
      '\x1d' -> KeyEvent (KChar ']') (mods <> CTRL)
      '\x1e' -> KeyEvent (KChar '^') (mods <> CTRL)
      '\x1f' -> KeyEvent (KChar '_') (mods <> CTRL)
      '\x7f' -> KeyEvent KBackspace mods
      '\x9b' -> KeyEvent KEsc (mods <> SHIFT)
      _
        -- '\x01' ..= '\x1a' (minus the ones handled above) are Ctrl-letters
        | c >= '\x01' && c <= '\x1a' ->
            KeyEvent (KChar (toEnum (fromEnum c + 0x40))) (mods <> CTRL)
        | otherwise -> KeyEvent KNull mods

-- | Constructor from a character with the Ctrl modifier.
ctrl :: Char -> KeyEvent
ctrl c = newKey c CTRL

-- | Constructor from a character with the Alt modifier.
alt :: Char -> KeyEvent
alt c = newKey c ALT

-- | @From<char> for KeyEvent@
fromChar :: Char -> KeyEvent
fromChar c = newKey c NONE

-- | ctrl-a => ctrl-A (uppercase); shift-A => A (no SHIFT modifier);
--   shift-Tab => 'KBackTab'.
normalize :: KeyEvent -> KeyEvent
normalize e@(KeyEvent code m) = case code of
  KChar c
    | isAsciiControl c -> newKey c m
    | isAsciiLower c && hasMods m CTRL -> KeyEvent (KChar (toUpper c)) m
    | isAsciiUpper c && hasMods m SHIFT -> KeyEvent (KChar c) (xorMods m SHIFT)
  KTab | hasMods m SHIFT -> KeyEvent KBackTab (xorMods m SHIFT)
  _ -> e
  where
    isAsciiControl ch = ch < '\x20' || ch == '\x7f'
