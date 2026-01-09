{-# LANGUAGE FlexibleInstances #-}
{-# LANGUAGE MultiParamTypeClasses #-}

module Idyllic.Build.Pipeline.Effect (InputMode (..), PipelineEnv (..), newPipelineEnv, defaultPipelineEnv, Pipeline) where

import Control.Concurrent.STM (TVar, newTVarIO)
import Control.Monad.Reader (ReaderT)
import Data.ByteString (ByteString)
import Data.Text (Text)
import Error.Diagnose (Diagnostic)
import Idyllic.Utils.LineIndex (LineIndex, buildLineIndex)

data InputMode
  = InputModeFile Text
  | InputModeInteractive
  deriving (Show, Eq)

data PipelineEnv = PipelineEnv
  { pipelineSrc :: ByteString,
    pipelineDebug :: Bool,
    pipelineMode :: InputMode,
    pipelineLineIndex :: LineIndex,
    pipelineErrors :: TVar [Diagnostic Text]
  }
  deriving (Eq)

newPipelineEnv :: Bool -> InputMode -> ByteString -> IO PipelineEnv
newPipelineEnv debug mode src = do
  errVar <- newTVarIO []
  pure
    PipelineEnv
      { pipelineSrc = src,
        pipelineDebug = debug,
        pipelineMode = mode,
        pipelineLineIndex = buildLineIndex src,
        pipelineErrors = errVar
      }

defaultPipelineEnv :: ByteString -> IO PipelineEnv
defaultPipelineEnv src = do
  errVar <- newTVarIO []
  pure
    PipelineEnv
      { pipelineSrc = src,
        pipelineDebug = True,
        pipelineMode = InputModeInteractive,
        pipelineLineIndex = buildLineIndex src,
        pipelineErrors = errVar
      }

type Pipeline = ReaderT PipelineEnv IO