module Idyllic.Pipeline (runPipelineIO) where

import Control.Monad.Reader (MonadIO (liftIO), MonadReader (ask), ReaderT (runReaderT), asks)
import Data.ByteString (ByteString)
import qualified Data.ByteString.Lazy as BL
import Data.Text
import Data.Text.Encoding
import Debug.Trace (trace)
import Error.Diagnose
import Error.Diagnose.Compat.Megaparsec
import Error.Diagnose.Diagnostic
import Idyllic.Effect
import Idyllic.Lexer
import Idyllic.Parser
import Text.Pretty.Simple

pipelineMain :: Pipeline ()
pipelineMain = do
  env <- ask
  let src = decodeUtf8 $ pipelineSrc env
  -- let result = parseInteractive src
  -- case result of
  --   Left e ->
  --     let diag = errorDiagnosticFromBundle Nothing ("Parse error on input" :: Text) Nothing e
  --         diag' = addFile diag "<interactive>" (unpack src)
  --      in liftIO $ printDiagnostic stderr True True 2 defaultStyle diag'
  --   Right ast -> pPrint ast
  pure ()

runPipelineIO :: Bool -> InputMode -> ByteString -> IO ()
runPipelineIO debug mode src = do
  env <- newPipelineEnv debug mode src
  runReaderT pipelineMain env