module Idyllic.Pipeline (runPipelineIO, newPipelineEnv, defaultPipelineEnv, PipelineEnv (..), InputMode (..)) where

import Control.Monad.Reader (MonadIO (liftIO), MonadReader (ask), ReaderT (runReaderT), asks)
import Data.IORef
import Data.Text (Text, pack)
import Data.Text qualified as T
import Data.Text.Lazy (toStrict)
import Data.Text.Lazy qualified as TL
import Error.Diagnose
import Error.Diagnose.Compat.Megaparsec
import Idyllic.Syntax.Layout
import Idyllic.Syntax.Lexer
import Idyllic.Syntax.Parser
import Idyllic.Utils
import Text.Megaparsec (errorBundlePretty)
import Text.Pretty.Simple

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

type PipelineM = ReaderT PipelineEnv IO

pipelineMain :: PipelineM String
pipelineMain = do
  env <- ask
  li <- asks pipelineLineIndex
  let src = pipelineSrc env
  case tokenize src of
    Left es -> pure $ errorBundlePretty es
    Right ts -> pure $ TL.unpack $ pShow ts

--     (lts, []) -> case parseInteractive lts of
--       Left es -> pPrint es
--       Right ast -> do
--         pPrint ast
--     (lts, es) -> liftIO $ do
--       putStrLn "Layout errors:"
--       mapM_ print es
--       case parseInteractive lts of
--         Left es' -> do
--           putStrLn "Parse errors:"
--           pPrint es'
--         Right ast -> pPrint ast

runPipelineIO :: Bool -> InputMode -> Text -> IO String
runPipelineIO debug mode src = do
  env <- newPipelineEnv debug mode src
  runReaderT pipelineMain env