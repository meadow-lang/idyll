-- | Common TTY interfaces (@tty/mod.rs@).
--
--   Rust's traits become records of functions so the editor can run against
--   the POSIX terminal or a scripted test terminal.
module System.Console.Rustyline.Tty
  ( TtyEvent (..),
    Buffer,
    KeyMap,
    RawReader (..),
    Renderer (..),
    computeLayout,
    escWidth,
    ExternalPrinter (..),
    Terminal (..),
    isUnsupportedTerm,
  )
where

import Data.ByteString (ByteString)
import Data.Char (isDigit, toLower)
import Data.Map.Strict (Map)
import Data.Text (Text)
import System.Console.Rustyline.Config (Config)
import System.Console.Rustyline.Highlight (Highlighter)
import System.Console.Rustyline.Keymap.Types (Cmd)
import System.Console.Rustyline.Keys (KeyEvent)
import System.Console.Rustyline.Layout
import System.Console.Rustyline.LineBuffer (LineBuffer, asStr, len, pos)
import System.Console.Rustyline.Internal.Text (dropB, takeB)
import System.Console.Rustyline.Prompt (Prompt)
import System.Environment (lookupEnv)
import Data.Text qualified as T

-- | Input event.
data TtyEvent
  = KeyPress KeyEvent
  | ExternalPrint Text
  | -- | macOS only: a timeout while waiting for input
    Timeout Bool

-- | Typeahead bytes kept between two @readline@ calls.
type Buffer = ByteString

-- | Terminal-specific bindings (from termios special characters).
type KeyMap = Map KeyEvent Cmd

-- | Translates bytes read from the input to keys.
data RawReader = RawReader
  { -- | Blocking wait for either a key press or an external print.
    waitForInput :: Bool -> IO TtyEvent,
    -- | Blocking read of the key pressed (the flag: a single ESC aborts).
    nextKey :: Bool -> IO KeyEvent,
    -- | For Ctrl-V support.
    nextChar :: IO Char,
    -- | Bracketed paste.
    readPastedText :: IO Text,
    -- | Is the key bound to a peculiar command?
    findBinding :: KeyEvent -> Maybe Cmd,
    -- | Back up typeahead.
    unbuffer :: IO (Maybe Buffer),
    -- | Is input available within the timeout (milliseconds, negative waits
    --   forever)? Used to query the cursor position.
    pollInput :: Int -> IO Bool,
    -- | Release resources (signal handlers); Rust does this on drop.
    releaseReader :: IO ()
  }

-- | Displays the prompt, line and cursor.
data Renderer = Renderer
  { moveCursor :: Position -> Position -> IO (),
    -- | Display the prompt, line and cursor. Arguments: prompt, line, hint,
    --   old layout (used to clear old rows), new layout, highlighter.
    refreshLine :: Prompt -> LineBuffer -> Maybe Text -> Maybe Layout -> Layout -> Maybe Highlighter -> IO (),
    -- | Number of columns and rows used to display the text on the terminal
    --   starting at the given position.
    calculatePosition :: Text -> Position -> IO Position,
    writeAndFlush :: Text -> IO (),
    -- | Used for completion when there is nothing to complete or when all
    --   the choices were already shown.
    beep :: IO (),
    -- | Clear the screen (Ctrl-L).
    clearScreen :: IO (),
    -- | Clear rows used by prompt and edited line.
    clearRows :: Layout -> IO (),
    -- | Clear from cursor to the end of line.
    clearToEol :: IO (),
    -- | Update the number of columns/rows of the terminal.
    updateSize :: IO (),
    getColumns :: IO Unit,
    getRows :: IO Unit,
    -- | Does the output support colours?
    colorsEnabled :: Bool,
    -- | How grapheme clusters are rendered.
    rendererGcm :: GraphemeClusterMode,
    -- | Make sure the prompt is at the leftmost edge of the screen.
    moveCursorAtLeftmost :: RawReader -> IO (),
    beginSynchronizedUpdate :: IO (),
    endSynchronizedUpdate :: IO ()
  }

-- | Layout for rendering prompt + line + some info (hint, validation
--   message, ...), with line wrapping depending on the screen width.
computeLayout :: Renderer -> Position -> Bool -> LineBuffer -> Maybe Text -> IO Layout
computeLayout r promptSize defaultPrompt line info = do
  let p = pos line
      text = asStr line
  -- calculate the desired position of the cursor
  cursor <- calculatePosition r (takeB p text) promptSize
  -- calculate the position of the end of the input line
  end0 <-
    if p == len line
      then pure cursor
      else calculatePosition r (dropB p text) cursor
  end <- maybe (pure end0) (\i -> calculatePosition r i end0) info
  pure
    Layout
      { layoutGcm = rendererGcm r,
        layoutPromptSize = promptSize,
        layoutDefaultPrompt = defaultPrompt,
        layoutCursor = cursor,
        layoutEnd = end,
        layoutHasInfo = info /= Nothing
      }

-- | Width of a grapheme, ignoring ANSI escape sequences. The state is 0
--   outside a sequence, 1 after ESC and 2 inside a CSI.
escWidth :: GraphemeClusterMode -> Text -> Int -> (Unit, Int)
escWidth gcm s escSeq
  | escSeq == 1 = (0, if s == "[" then 2 else 0) -- CSI or two-character sequence
  | escSeq == 2 =
      if s == ";" || maybe False (isDigit . fst) (T.uncons s)
        then (0, 2)
        else (0, 0) -- not supported
  | s == "\x1b" = (0, 1)
  | s == "\n" = (0, escSeq)
  | otherwise = (gcmWidth gcm s, escSeq)

-- | Prints above the edited line from other threads.
newtype ExternalPrinter = ExternalPrinter
  { -- | Print a message to stdout.
    externalPrinterPrint :: Text -> IO ()
  }

-- | Terminal contract.
data Terminal = Terminal
  { -- | Can the terminal provide a rich line-editing user interface?
    termIsUnsupported :: Bool,
    -- | Is input connected to a terminal?
    termIsInputTty :: Bool,
    -- | Is output connected to a terminal?
    termIsOutputTty :: Bool,
    -- | Enable raw mode. Returns the action that restores the previous mode
    --   and the terminal key map.
    enableRawMode :: Config -> IO (IO (), KeyMap),
    createReader :: Maybe Buffer -> Config -> KeyMap -> IO RawReader,
    createWriter :: Config -> IO Renderer,
    writeln :: IO (),
    createExternalPrinter :: IO ExternalPrinter,
    -- | Change cursor visibility; returns the action restoring it, if any.
    setCursorVisibility :: Bool -> IO (Maybe (IO ())),
    -- | Suspend the process (Ctrl-Z).
    suspendProcess :: IO (),
    -- | Rust compiles some bindings (Ctrl-C as 'Interrupt', Ctrl-D on an
    --   empty line as 'EndOfFile') only for tests and Windows.
    termTestBindings :: Bool
  }

-- | Is @TERM@ one of the terminals without raw mode support?
isUnsupportedTerm :: IO Bool
isUnsupportedTerm = do
  term <- lookupEnv "TERM"
  pure $ case term of
    Just t -> map toLower t `elem` ["dumb", "cons25", "emacs"]
    Nothing -> False
