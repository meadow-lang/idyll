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
      debug = cfgDebug (peConfig env)
      (toks, lexDiags) = tokenize src
      (laidOut, layoutErrs) = layout li toks
  diagsRef <- asks peErrors
  let record ds = liftIO (modifyIORef' diagsRef (<> ds))
  record (lexDiags <> map (layoutErrorReport li) layoutErrs)
  body <- case parseInteractive src laidOut of
    Left e -> pure (prettyParseError e)
    Right ast -> do
      -- name resolution -> HIR, then SCC / retopologize, then type check
      let RenameResult {rrModule, rrSpans, rrErrors} = renameModule ast
          hir = sccModule rrModule
          TypeCheckResult {tcrBodyType, tcrErrors} = checkModule hir
      record (map (renameErrorReport li) rrErrors)
      record (map (typeErrorReport li rrSpans) tcrErrors)
      pure $
        (if debug then TL.unpack (pShow hir) <> "\n" else "")
          <> ": "
          <> T.unpack (showCore tcrBodyType)
  -- render everything the passes reported, against the original source
  diags <- liftIO (readIORef diagsRef)
  pure $ case diags of
    [] -> body
    _ -> renderDiagnostics src diags <> "\n" <> body

-- | Pretty-print the accumulated reports, with the source attached so
--   'Error.Diagnose' can show the offending lines.
renderDiagnostics :: Text -> [Report Text] -> String
renderDiagnostics src reports =
  renderString
    . layoutPretty defaultLayoutOptions
    . unAnnotate
    $ prettyDiagnostic False 4 diagnostic
  where
    diagnostic =
      foldl addReport (addFile (def :: Diagnostic Text) "<input>" (T.unpack src)) reports

layoutErrorReport :: LineIndex -> LayoutError -> Report Text
layoutErrorReport li e = case e of
  NestedLayoutContextError outer inner sp ->
    mk sp $
      "nested layout context: expected indentation greater than "
        <> tshow outer
        <> ", but got "
        <> tshow inner
  UnexpectedClosingBraceError sp -> mk sp "unexpected closing brace"
  NotInLayoutContextAtEndOfInputError sp ->
    mk sp "not inside a layout context at end of input"
  where
    tshow :: (Show a) => a -> Text
    tshow = T.pack . show
    mk (Span s eoff) msg =
      let (l1, c1) = offsetToLineCol li s
          (l2, c2) = offsetToLineCol li eoff
       in err Nothing msg [(Position (l1, c1) (l2, c2) "<input>", This msg)] []

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
