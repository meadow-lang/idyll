module Idyllic.Pipeline (runPipelineIO) where

import Control.Monad.Reader (MonadIO (liftIO), MonadReader (ask), ReaderT (runReaderT), asks)
import Data.Text
import Error.Diagnose
import Error.Diagnose.Compat.Megaparsec
import Idyllic.Effect
import Idyllic.Layout
import Idyllic.Lexer
import Idyllic.Parser
import Text.Megaparsec (errorBundlePretty)
import Text.Pretty.Simple

pipelineMain :: Pipeline ()
pipelineMain = do
  env <- ask
  li <- asks pipelineLineIndex
  let src = pipelineSrc env
  case tokenize src of
    Left es -> liftIO $ print $ errorBundlePretty es
    -- Right ts -> do
    --   -- pPrint ts
    --   pPrint $ mark li ts
    --   pPrint $ layout li ts
    Right ts -> case parseInteractive $ layout li ts of
      Left es -> do
        pPrint $ layout li ts
        pPrint es
      -- let diag = errorDiagnosticFromBundle Nothing ("Parse error on input" :: Text) Nothing es
      --     diag' = addFile diag "<interactive>" (unpack src)
      --  in liftIO $ printDiagnostic stderr True True 2 defaultStyle diag'
      Right ast -> do
        -- pPrint $ mark li ts
        pPrint $ layout li ts
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