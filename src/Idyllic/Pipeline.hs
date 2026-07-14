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

data PipelineEnv = PipelineEnv
  { peSrc :: Text,
    peConfig :: Config,
    peLineIndex :: LineIndex,
    peErrors :: IORef [Report Text]
  }
  deriving (Eq)

newPipelineEnv :: Bool -> InputMode -> Text -> IO PipelineEnv
newPipelineEnv debug mode src = do
  errVar <- newIORef []
  pure
    PipelineEnv
      { peSrc = src,
        peConfig = Config debug mode,
        peLineIndex = buildLineIndex src,
        peErrors = errVar
      }

defaultPipelineEnv :: Text -> IO PipelineEnv
defaultPipelineEnv src = do
  errVar <- newIORef []
  pure
    PipelineEnv
      { peSrc = src,
        peConfig = Config False InputModeInteractive,
        peLineIndex = buildLineIndex src,
        peErrors = errVar
      }

data InputMode
  = InputModeFile Text
  | InputModeInteractive
  deriving (Show, Eq)

data Config = Config
  { cfgDebug :: Bool,
    cfgMode :: InputMode
  }
  deriving (Show, Eq)

type PipelineM = ReaderT PipelineEnv IO

pipeline :: PipelineM String
pipeline = do
  env <- ask
  li <- asks peLineIndex
  let src = peSrc env
  case tokenize src of
    Left es -> pure $ errorBundlePretty es
    Right ts -> pure $ TL.unpack $ pShow ts

runPipelineIO :: Bool -> InputMode -> Text -> IO String
runPipelineIO debug mode src = do
  env <- newPipelineEnv debug mode src
  runReaderT pipeline env

type Pass input out = Config -> input -> IO (out, [Report Text])

runPass :: Pass input out -> input -> PipelineM out
runPass run input = do
  cfg <- asks peConfig
  diagsRef <- asks peErrors
  (result, diags) <- liftIO (run cfg input)
  liftIO (modifyIORef' diagsRef (<> diags))
  pure result
