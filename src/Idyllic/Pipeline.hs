module Idyllic.Pipeline (runPipelineIO, newPipelineEnv, defaultPipelineEnv, PipelineEnv (..), InputMode (..)) where

import Control.Monad.Reader (MonadIO (liftIO), MonadReader (ask), ReaderT (runReaderT), asks)
import Data.IORef
import Data.Text (Text)
import Data.Text qualified as T
import Data.Text.Lazy qualified as TL
import Error.Diagnose
import Error.Diagnose.Compat.Megaparsec ()
import Idyllic.Rename.Env (renameErrorReport)
import Idyllic.Rename.Pass (RenameResult (..), renameModule)
import Idyllic.Scc (sccModule)
import Idyllic.Syntax.Layout
import Idyllic.Syntax.Lexer
import Idyllic.Syntax.Parser
import Idyllic.TypeCheck (TypeCheckResult (..), checkModule, showCore, typeErrorReport)
import Idyllic.Utils
import Prettyprinter (defaultLayoutOptions, layoutPretty, unAnnotate)
import Prettyprinter.Render.String (renderString)
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
  let src = peSrc env
      li = peLineIndex env
      cfg = peConfig env
      file = inputName (cfgMode cfg)
      (toks, lexDiags) = tokenizeNamed file src
      (laidOut, layoutErrs) = layout li toks
  diagsRef <- asks peErrors
  let record ds = liftIO (modifyIORef' diagsRef (<> ds))
  record (lexDiags <> map (layoutErrorReport file li) layoutErrs)
  -- A parse failure is reported like any other pass and does not short
  -- circuit: whatever the lexer and layout already found still gets shown.
  (body, parseDiag) <- case parseTerm file src laidOut of
    Left e -> pure ("", parseErrorDiagnostic e)
    Right ast -> do
      -- name resolution -> HIR, then SCC / retopologize, then type check
      let RenameResult {rrModule, rrSpans, rrErrors} = renameModule ast
          hir = sccModule rrModule
          TypeCheckResult {tcrBodyType, tcrErrors} = checkModule hir
      record (map (renameErrorReport file li) rrErrors)
      record (map (typeErrorReport file li rrSpans) tcrErrors)
      pure
        ( (if cfgDebug cfg then TL.unpack (pShow hir) <> "\n" else "")
            <> ": "
            <> T.unpack (showCore tcrBodyType),
          def
        )
  reports <- liftIO (readIORef diagsRef)
  -- reports first, then parseDiag: diagnostics come out in pass order
  pure (render file src (foldl addReport def reports <> parseDiag) body)

-- | The name every pass attributes its diagnostics to.  It has to agree with
--   the name 'addFile' registers the source under, or 'Error.Diagnose' cannot
--   find the lines to quote.
inputName :: InputMode -> FilePath
inputName (InputModeFile f) = T.unpack f
inputName InputModeInteractive = "<interactive>"

-- | Render every pass's diagnostics through 'Error.Diagnose', with the source
--   attached so the offending lines can be quoted, then the pass output.
render :: FilePath -> Text -> Diagnostic Text -> String -> String
render file src diag body
  | null rendered = body
  | null body = rendered
  | otherwise = rendered <> "\n" <> body
  where
    rendered =
      renderString
        . layoutPretty defaultLayoutOptions
        . unAnnotate
        $ prettyDiagnostic False 4 (addFile diag file (T.unpack src))

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
