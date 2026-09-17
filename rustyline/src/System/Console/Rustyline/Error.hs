-- | Error type for readline operations (@error.rs@).
module System.Console.Rustyline.Error
  ( ReadlineError (..),
    Signal (..),
  )
where

import Control.Exception (Exception, IOException)

-- | Signal received from the terminal.
data Signal
  = -- | SIGINT
    SigInterrupt
  | -- | SIGWINCH
    SigResize
  deriving (Eq, Show)

-- | The ways a call to @readline@ can end without a line.
data ReadlineError
  = -- | I/O (or system call) error
    ReadlineIo IOException
  | -- | EOF (VEOF / Ctrl-D)
    Eof
  | -- | Interrupt signal (VINTR / VQUIT / Ctrl-C)
    Interrupted
  | -- | A signal that arrived while waiting for input
    ReadlineSignal Signal
  deriving (Show)

instance Exception ReadlineError
