{-# LANGUAGE FlexibleInstances #-}
{-# LANGUAGE MultiParamTypeClasses #-}

module Idyllic.Effect (InputMode (..), PipelineEnv (..), newPipelineEnv, defaultPipelineEnv, Pipeline) where

import Control.Monad.Reader (ReaderT)
import Data.ByteString (ByteString)
import Data.IORef (IORef)
import Data.Text (Text)
import Error.Diagnose (Diagnostic)
import GHC.IORef (newIORef)
import Idyllic.Utils (LineIndex, buildLineIndex)

data InputMode
  = InputModeFile Text
  | InputModeInteractive
  deriving (Show, Eq)

data PipelineEnv = PipelineEnv
  { pipelineSrc :: Text,
    pipelineDebug :: Bool,
    pipelineMode :: InputMode,
    pipelineLineIndex :: LineIndex,
    pipelineErrors :: IORef [Diagnostic Text]
  }
  deriving (Eq)

newPipelineEnv :: Bool -> InputMode -> Text -> IO PipelineEnv
newPipelineEnv debug mode src = do
  errVar <- newIORef []
  pure
    PipelineEnv
      { pipelineSrc = src,
        pipelineDebug = debug,
        pipelineMode = mode,
        pipelineLineIndex = buildLineIndex src,
        pipelineErrors = errVar
      }

defaultPipelineEnv :: Text -> IO PipelineEnv
defaultPipelineEnv src = do
  errVar <- newIORef []
  pure
    PipelineEnv
      { pipelineSrc = src,
        pipelineDebug = True,
        pipelineMode = InputModeInteractive,
        pipelineLineIndex = buildLineIndex src,
        pipelineErrors = errVar
      }

type Pipeline = ReaderT PipelineEnv IO