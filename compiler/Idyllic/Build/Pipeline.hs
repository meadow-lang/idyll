module Idyllic.Build.Pipeline (runPipelineIO) where

import Control.Monad.Reader (MonadReader (ask), ReaderT (runReaderT), asks)
import Data.ByteString (ByteString)
import qualified Data.ByteString.Lazy as BL
import Debug.Trace (trace)
import Idyllic.Build.Pipeline.Effect
import Idyllic.Syn.Layout (layout)
import Idyllic.Syn.Lexer (tokenize)
import Text.Pretty.Simple

pipelineMain :: Pipeline ()
pipelineMain = do
  env <- ask
  let src = pipelineSrc env
  let ts = tokenize (BL.fromStrict src)
  trace "tokens" $ pPrint ts
  li <- asks pipelineLineIndex
  let l = layout li src ts
  trace "layouted tokens" $ pPrint l
  pure ()

runPipelineIO :: Bool -> InputMode -> ByteString -> IO ()
runPipelineIO debug mode src = do
  env <- mkPipelineEnv debug mode src
  runReaderT pipelineMain env