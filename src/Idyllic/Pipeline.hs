module Idyllic.Pipeline
  ( runPipelineIO,
    newPipelineEnv,
    defaultPipelineEnv,
    PipelineEnv (..),
    InputMode (..),
    OutputStyle (..),
    detectOutputStyle,
  )
where

import Control.Monad.Reader (MonadIO (liftIO), MonadReader (ask), ReaderT (runReaderT), asks)
import Data.Char (toUpper)
import Data.IORef
import Data.List (isPrefixOf)
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
import Prettyprinter.Render.Terminal (renderStrict)
import System.Environment (lookupEnv)
import System.IO (Handle, hGetEncoding, hIsTerminalDevice)
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
  style <- detectOutputStyle stdout
  pure
    PipelineEnv
      { peSrc = src,
        peConfig = Config debug mode style,
        peLineIndex = buildLineIndex src,
        peErrors = errVar
      }

defaultPipelineEnv :: Text -> IO PipelineEnv
defaultPipelineEnv src = newPipelineEnv False InputModeInteractive src

data InputMode
  = InputModeFile Text
  | InputModeInteractive
  deriving (Show, Eq)

data Config = Config
  { cfgDebug :: Bool,
    cfgMode :: InputMode,
    cfgStyle :: OutputStyle
  }
  deriving (Show, Eq)

-- | What the terminal we are printing to can actually render.
data OutputStyle = OutputStyle
  { -- | the handle's encoding can represent diagnose's box-drawing gutters
    osUnicode :: Bool,
    -- | ANSI colour is wanted and understood
    osColor :: Bool
  }
  deriving (Show, Eq)

-- | Ask a handle what it can render.
--
--   Unicode is a /capability/ check rather than a preference: diagnose draws
--   its gutters with box-drawing characters, and writing those to a handle
--   whose encoding cannot represent them throws, so we ask the handle for its
--   encoding rather than guessing.
--
--   Colour follows the usual conventions — on for a terminal, off for a pipe
--   or @TERM=dumb@, with @NO_COLOR@ to force it off and @FORCE_COLOR@ /
--   @CLICOLOR_FORCE@ to force it on (useful when piping into a pager that
--   understands escapes).
detectOutputStyle :: Handle -> IO OutputStyle
detectOutputStyle h = do
  encoding <- hGetEncoding h
  isTty <- hIsTerminalDevice h
  term <- lookupEnv "TERM"
  noColor <- lookupEnv "NO_COLOR"
  forced <- anySet ["FORCE_COLOR", "CLICOLOR_FORCE"]
  let colour
        | isSet noColor = False
        | forced = True
        | otherwise = isTty && term /= Just "dumb"
  pure
    OutputStyle
      { -- the Show instance for TextEncoding is its name, e.g. "UTF-8"
        osUnicode = maybe False (isPrefixOf "UTF" . map toUpper . show) encoding,
        osColor = colour
      }
  where
    -- the NO_COLOR convention: set but empty does not count as set
    isSet = maybe False (not . null)
    anySet = fmap (any isSet) . traverse lookupEnv

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
  pure (render (cfgStyle cfg) file src (foldl addReport def reports <> parseDiag) body)

-- | The name every pass attributes its diagnostics to.  It has to agree with
--   the name 'addFile' registers the source under, or 'Error.Diagnose' cannot
--   find the lines to quote.
inputName :: InputMode -> FilePath
inputName (InputModeFile f) = T.unpack f
inputName InputModeInteractive = "<interactive>"

-- | Render every pass's diagnostics through 'Error.Diagnose', with the source
--   attached so the offending lines can be quoted, then the pass output.
render :: OutputStyle -> FilePath -> Text -> Diagnostic Text -> String -> String
render style file src diag body
  | null rendered = body
  | null body = rendered
  | otherwise = rendered <> "\n" <> body
  where
    doc = prettyDiagnostic (osUnicode style) 4 (addFile diag file (T.unpack src))
    -- 'defaultStyle' resolves diagnose's abstract annotations ('ThisColor',
    -- 'RuleColor', ..) into ANSI escapes; 'unAnnotate' discards them instead.
    coloured
      | osColor style = defaultStyle doc
      | otherwise = unAnnotate doc
    rendered = T.unpack (renderStrict (layoutPretty defaultLayoutOptions coloured))

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
