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
    Right ts -> case layout li ts of
      (lts, []) -> case parseInteractive lts of
        Left es -> do
          pPrint $ mark li ts
          pPrint lts
          pPrint es
        Right ast -> do
          pPrint $ mark li ts
      -- pPrint lts
      -- pPrint ast
      (lts, es) -> liftIO $ do
        putStrLn "Layout errors:"
        mapM_ print es
        case parseInteractive lts of
          Left es' -> do
            putStrLn "Parse errors:"
            pPrint es'
          Right ast -> pPrint ast
  pure ()

runPipelineIO :: Bool -> InputMode -> Text -> IO ()
runPipelineIO debug mode src = do
  env <- newPipelineEnv debug mode src
  runReaderT pipelineMain env