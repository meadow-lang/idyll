-- | Custom event handlers (@binding.rs@).
--
--   Rust stores bindings in a radix trie keyed by the big-endian encoding of
--   each key; here a 'Map' keyed by the same encoding gives the same prefix
--   queries.
module System.Console.Rustyline.Binding
  ( Event (..),
    normalizeEvent,
    eventGet,
    keySeq,
    EventHandler (..),
    EventContext (..),
    ConditionalEventHandler,
    Bindings,
    emptyBindings,
    insertBinding,
    removeBinding,
    lookupBinding,
    hasDescendant,
    encodeKey,
  )
where

import Data.Bits ((.|.))
import Data.List (isPrefixOf)
import Data.Map.Strict (Map)
import Data.Map.Strict qualified as Map
import Data.Maybe (listToMaybe)
import Data.Text (Text)
import Data.Word (Word32)
import System.Console.Rustyline.Config (EditMode)
import System.Console.Rustyline.Keymap.Types (Cmd, InputMode, RepeatCount)
import System.Console.Rustyline.Keys

-- | Input event.
data Event
  = -- | Wildcard. Useful if you want to filter out some keys.
    Any
  | -- | Key sequence
    KeySeq [KeyEvent]
  | -- | Mouse event (not yet supported)
    Mouse
  deriving (Eq, Show)

-- | See 'normalize'.
normalizeEvent :: Event -> Event
normalizeEvent = \case
  KeySeq keys -> KeySeq (map normalize keys)
  e -> e

-- | The @i@th key event.
eventGet :: Int -> Event -> Maybe KeyEvent
eventGet i = \case
  KeySeq ks -> listToMaybe (drop i ks)
  _ -> Nothing

-- | A single-key event.
keySeq :: KeyEvent -> Event
keySeq k = KeySeq [k]

base, baseControl, baseMeta, baseShift, escapeCode :: Word32
base = 0x0010_ffff + 1
baseControl = 0x0200_0000
baseMeta = 0x0400_0000
baseShift = 0x0100_0000
escapeCode = 27

pageUp, pageDown, down, up, left, right, home, end, delete, insert, mouse, pasteStart, pasteFinish, anyCode :: Word32
pageUp = base + 1
pageDown = pageUp + 1
down = pageDown + 1
up = down + 1
left = up + 1
right = left + 1
home = right + 1
end = home + 1
delete = end + 1
insert = delete + 1
mouse = insert + 25
pasteStart = mouse + 1
pasteFinish = pasteStart + 1
anyCode = pasteFinish + 1

encodeKey :: KeyEvent -> Word32
encodeKey (KeyEvent code mods) = u .|. ctrlBit .|. metaBit .|. shiftBit
  where
    u = case code of
      KUnknownEscSeq -> 0
      KNull -> 0
      KBackspace -> 0x7f
      KBackTab -> 9 .|. baseShift
      KBracketedPasteStart -> pasteStart
      KBracketedPasteEnd -> pasteFinish
      KChar c -> fromIntegral (fromEnum c)
      KDelete -> delete
      KDown -> down
      KEnd -> end
      KEnter -> 13
      KF i -> insert + fromIntegral i
      KEsc -> escapeCode
      KHome -> home
      KInsert -> insert
      KLeft -> left
      KPageDown -> pageDown
      KPageUp -> pageUp
      KRight -> right
      KTab -> 9
      KUp -> up
    ctrlBit = if hasMods mods CTRL then baseControl else 0
    metaBit = if hasMods mods ALT then baseMeta else 0
    shiftBit = if hasMods mods SHIFT then baseShift else 0

encodeEvent :: Event -> [Word32]
encodeEvent = \case
  Any -> [anyCode]
  KeySeq keys -> map encodeKey keys
  Mouse -> [mouse]

-- | Snapshot of the editor given to conditional handlers.
data EventContext = EventContext
  { -- | emacs or vi mode
    ecMode :: EditMode,
    -- | vi input mode
    ecInputMode :: InputMode,
    -- | the hint text shown after the cursor, if a hint is displayed
    ecHintText :: Maybe Text,
    -- | whether a hint is displayed
    ecHasHint :: Bool,
    -- | currently edited line
    ecLine :: Text,
    -- | current cursor position (byte position)
    ecPos :: Int
  }

-- | Takes the event, the repeat count, its sign and the input state and
--   returns the command to perform, or 'Nothing' to perform the default one.
type ConditionalEventHandler = Event -> RepeatCount -> Bool -> EventContext -> IO (Maybe Cmd)

-- | Event handler.
data EventHandler
  = -- | unconditional command
    Simple Cmd
  | -- | handler behaviour depends on input state
    Conditional ConditionalEventHandler

newtype Bindings = Bindings (Map [Word32] EventHandler)

emptyBindings :: Bindings
emptyBindings = Bindings Map.empty

-- | Bind a (normalized) sequence to a handler, returning the previous one.
insertBinding :: Event -> EventHandler -> Bindings -> (Maybe EventHandler, Bindings)
insertBinding evt handler (Bindings m) =
  let k = encodeEvent (normalizeEvent evt)
   in (Map.lookup k m, Bindings (Map.insert k handler m))

-- | Remove a binding, returning it.
removeBinding :: Event -> Bindings -> (Maybe EventHandler, Bindings)
removeBinding evt (Bindings m) =
  let k = encodeEvent (normalizeEvent evt)
   in (Map.lookup k m, Bindings (Map.delete k m))

lookupBinding :: Event -> Bindings -> Maybe EventHandler
lookupBinding evt (Bindings m) = Map.lookup (encodeEvent evt) m

-- | Is some binding's key a (non-strict) extension of the event? (Rust's
--   @get_raw_descendant(evt).is_some()@)
hasDescendant :: Event -> Bindings -> Bool
hasDescendant evt (Bindings m) = case Map.lookupGE k m of
  Just (k', _) -> k `isPrefixOf` k'
  Nothing -> False
  where
    k = encodeEvent evt
