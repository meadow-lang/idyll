-- | Scripted terminal for tests (@tty/test.rs@).
module System.Console.Rustyline.Tty.Test
  ( DummyTerminal (..),
    newDummyTerminal,
    dummyTerminal,
    keysReader,
    sink,
  )
where

import Control.Exception (throwIO)
import Data.IORef
import System.Console.Rustyline.Error (ReadlineError (..))
import System.Console.Rustyline.Internal.Text (lenB)
import System.Console.Rustyline.Keys
import System.Console.Rustyline.Layout
import System.Console.Rustyline.Tty

-- | Keys to replay, and the cursor column before the last accept command.
data DummyTerminal = DummyTerminal
  { dummyKeys :: IORef [KeyEvent],
    dummyCursor :: IORef Int
  }

newDummyTerminal :: IO DummyTerminal
newDummyTerminal = DummyTerminal <$> newIORef [] <*> newIORef 0

-- | A reader that replays the keys, then reports end of file.
keysReader :: [KeyEvent] -> IO RawReader
keysReader keys = do
  ref <- newIORef keys
  let next =
        readIORef ref >>= \case
          (k : rest) -> k <$ writeIORef ref rest
          [] -> throwIO Eof
  pure
    RawReader
      { waitForInput = \_ -> KeyPress <$> next,
        nextKey = const next,
        nextChar =
          next >>= \case
            KeyEvent (KChar c) NONE -> pure c
            k -> error ("nextChar: unexpected key " <> show k),
        readPastedText = pure "pasted",
        findBinding = const Nothing,
        unbuffer = pure Nothing,
        pollInput = const (pure False),
        releaseReader = pure ()
      }

-- | A renderer that draws nothing; every byte is one column.
sink :: Renderer
sink =
  Renderer
    { moveCursor = \_ _ -> pure (),
      refreshLine = \_ _ _ _ _ _ -> pure (),
      calculatePosition = \s orig -> pure orig {posCol = posCol orig + lenB s},
      writeAndFlush = \_ -> pure (),
      beep = pure (),
      clearScreen = pure (),
      clearRows = \_ -> pure (),
      clearToEol = pure (),
      updateSize = pure (),
      getColumns = pure 80,
      getRows = pure 24,
      colorsEnabled = False,
      rendererGcm = Unicode,
      moveCursorAtLeftmost = \_ -> pure (),
      beginSynchronizedUpdate = pure (),
      endSynchronizedUpdate = pure ()
    }

-- | The terminal: input is a tty, output is not.
dummyTerminal :: DummyTerminal -> Terminal
dummyTerminal dt =
  Terminal
    { termIsUnsupported = False,
      termIsInputTty = True,
      termIsOutputTty = False,
      enableRawMode = \_ -> pure (pure (), mempty),
      createReader = \_ _ _ -> readIORef (dummyKeys dt) >>= keysReader,
      createWriter = \_ -> pure sink,
      writeln = pure (),
      createExternalPrinter = pure (ExternalPrinter (\_ -> pure ())),
      setCursorVisibility = \_ -> pure Nothing,
      suspendProcess = pure (),
      termTestBindings = True
    }
