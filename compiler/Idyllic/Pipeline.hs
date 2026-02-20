module Idyllic.Pipeline (runPipelineIO) where

import Control.Monad.Reader (MonadIO (liftIO), MonadReader (ask), ReaderT (runReaderT), asks)
import Data.Text
import Idyllic.Effect
import Idyllic.Lexer
import Idyllic.Parser
import Text.Pretty.Simple
import Text.Megaparsec (errorBundlePretty)

pipelineMain :: Pipeline ()
pipelineMain = do
  env <- ask
  let src = pipelineSrc env
  case tokenize src of
    Left es -> liftIO $ print $ errorBundlePretty es
    Right ts -> case parseInteractive ts of
      Left es -> pPrint es
      Right ast -> do
        pPrint ts
        pPrint ast
  -- let result = parseInteractive src
  -- case result of
  --   Left e ->
  --     let diag = errorDiagnosticFromBundle Nothing ("Parse error on input" :: Text) Nothing e
  --         diag' = addFile diag "<interactive>" (unpack src)
  --      in liftIO $ printDiagnostic stderr True True 2 defaultStyle diag'
  --   Right ast -> pPrint ast
  pure ()

runPipelineIO :: Bool -> InputMode -> Text -> IO ()
runPipelineIO debug mode src = do
  env <- newPipelineEnv debug mode src
  runReaderT pipelineMain env