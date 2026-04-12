module Idyllic.Schema where

import Data.Map (Map)
import Data.Text (Text)

-- data InputMode
--   = InputModeFile Text
--   | InputModeInteractive
--   deriving (Show, Eq)

-- data PipelineEnv = PipelineEnv
--   { pipelineSrc :: Text,
--     pipelineDebug :: Bool,
--     pipelineMode :: InputMode,
--     pipelineLineIndex :: LineIndex,
--     pipelineErrors :: TVar [Diagnostic Text]
--   }
--   deriving (Eq)

-- newPipelineEnv :: Bool -> InputMode -> Text -> IO PipelineEnv
-- newPipelineEnv debug mode src = do
--   errVar <- newTVarIO []
--   pure
--     PipelineEnv
--       { pipelineSrc = src,
--         pipelineDebug = debug,
--         pipelineMode = mode,
--         pipelineLineIndex = buildLineIndex src,
--         pipelineErrors = errVar
--       }

-- defaultPipelineEnv :: Text -> IO PipelineEnv
-- defaultPipelineEnv src = do
--   errVar <- newTVarIO []
--   pure
--     PipelineEnv
--       { pipelineSrc = src,
--         pipelineDebug = True,
--         pipelineMode = InputModeInteractive,
--         pipelineLineIndex = buildLineIndex src,
--         pipelineErrors = errVar
--       }