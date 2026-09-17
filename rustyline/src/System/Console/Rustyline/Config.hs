-- | User preferences (@config.rs@).
--
--   Rust's builder is replaced by plain record update on 'defaultConfig'
--   plus the @set*@ helpers below; accessors keep the Rust names.
module System.Console.Rustyline.Config
  ( Config (..),
    defaultConfig,
    BellStyle (..),
    HistoryDuplicates (..),
    CompletionType (..),
    EditMode (..),
    ColorMode (..),
    Behavior (..),
    GraphemeClusterMode (..),
    graphemeClusterModeFromEnv,
    effectiveColorMode,
    resolveGraphemeClusterMode,
    withHistoryIgnoreDups,
    withEditMode,
  )
where

import Data.Word (Word16, Word8)
import System.Environment (lookupEnv)

-- | Tell how grapheme clusters are supported / rendered (@layout.rs@).
data GraphemeClusterMode
  = -- | Support grapheme clustering
    Unicode
  | -- | Doesn't support shaping
    WcWidth
  | -- | Skip zero-width joiner
    NoZwj
  deriving (Eq, Show)

-- | Use environment variables to guess how the terminal renders graphemes
--   (@GraphemeClusterMode::from_env@).
graphemeClusterModeFromEnv :: IO GraphemeClusterMode
graphemeClusterModeFromEnv = do
  termProgram <- lookupEnv "TERM_PROGRAM"
  case termProgram of
    Just "Apple_Terminal" -> pure Unicode
    Just "iTerm.app" -> pure Unicode
    Just "WezTerm" -> pure Unicode
    Nothing -> do
      term <- lookupEnv "TERM"
      pure $ case term of
        Just "xterm-kitty" -> NoZwj
        _ -> WcWidth
    Just _ -> pure WcWidth

-- | Beep or flash or nothing.
data BellStyle
  = -- | Beep
    Audible
  | -- | Silent
    BellNone
  | -- | Flash screen (not supported)
    Visible
  deriving (Eq, Show)

-- | History filter.
data HistoryDuplicates
  = -- | No filter
    AlwaysAdd
  | -- | a line will not be added to the history if it matches the previous entry
    IgnoreConsecutive
  deriving (Eq, Show)

-- | Tab completion style.
data CompletionType
  = -- | Complete the next full match (like in Vim by default)
    Circular
  | -- | Complete till longest match. When more than one match, list all
    --   matches (like in Bash/Readline).
    List
  deriving (Eq, Show)

-- | Style of editing / standard keymaps.
data EditMode
  = -- | Emacs keymap
    Emacs
  | -- | Vi keymap
    Vi
  deriving (Eq, Show)

-- | Colorization mode.
data ColorMode
  = -- | Activate highlighting if platform/terminal is supported.
    Enabled
  | -- | Activate highlighting even if platform is not supported.
    Forced
  | -- | Deactivate highlighting even if platform/terminal is supported.
    Disabled
  deriving (Eq, Show)

-- | Should the editor use stdio.
data Behavior
  = -- | Use stdin / stdout
    Stdio
  | -- | Use terminal-style interaction whenever possible, even if 'stdin'
    --   and/or 'stdout' are not terminals.
    PreferTerm
  deriving (Eq, Show)

data Config = Config
  { -- | Maximum number of entries in History.
    maxHistorySize :: Int,
    -- | Whether lines matching the previous history entry are saved.
    historyDuplicates :: HistoryDuplicates,
    -- | Whether lines which begin with a space are ignored by the history.
    historyIgnoreSpace :: Bool,
    completionType :: CompletionType,
    -- | Directly show all alternatives with 'List' completion.
    completionShowAllIfAmbiguous :: Bool,
    -- | Ask before listing more than this many completions.
    completionPromptLimit :: Int,
    -- | Milliseconds to wait for a character when reading an ambiguous key
    --   sequence ('Nothing' waits forever).
    keyseqTimeout :: Maybe Word16,
    -- | Emacs or Vi mode. Use 'withEditMode' to also adjust 'keyseqTimeout'.
    editMode :: EditMode,
    -- | Add each nonblank line returned by @readline@ to the history.
    autoAddHistory :: Bool,
    bellStyle :: BellStyle,
    -- | Whether colours should be enabled (see 'effectiveColorMode').
    colorMode :: ColorMode,
    -- | How the terminal renders grapheme clusters; 'Nothing' means guess
    --   from the environment (Rust's default does this eagerly).
    graphemeClusterMode :: Maybe GraphemeClusterMode,
    -- | Whether to use stdio or not.
    behavior :: Behavior,
    -- | Horizontal space taken by a tab.
    tabStop :: Word8,
    -- | Indentation size for indent/dedent commands.
    indentSize :: Word8,
    -- | Check that the cursor is at the leftmost column before the prompt.
    checkCursorPosition :: Bool,
    -- | Bracketed paste on unix platform.
    enableBracketedPaste :: Bool,
    -- | Synchronized output on unix platform.
    enableSynchronizedOutput :: Bool,
    -- | Whether to keep terminal signals (ISIG) enabled.
    enableSignals :: Bool
  }
  deriving (Eq, Show)

defaultConfig :: Config
defaultConfig =
  Config
    { maxHistorySize = 100,
      historyDuplicates = IgnoreConsecutive,
      historyIgnoreSpace = False,
      completionType = Circular,
      completionPromptLimit = 100,
      completionShowAllIfAmbiguous = False,
      keyseqTimeout = Nothing,
      editMode = Emacs,
      autoAddHistory = False,
      bellStyle = Audible,
      colorMode = Enabled,
      graphemeClusterMode = Nothing,
      behavior = Stdio,
      tabStop = 8,
      indentSize = 2,
      checkCursorPosition = False,
      enableBracketedPaste = True,
      enableSynchronizedOutput = True,
      enableSignals = False
    }

-- | @Config::color_mode@: 'Enabled' is downgraded to 'Disabled' when
--   @NO_COLOR@ is set to a non-empty value.
effectiveColorMode :: Config -> IO ColorMode
effectiveColorMode c = case colorMode c of
  Enabled -> do
    noColor <- lookupEnv "NO_COLOR"
    pure $ case noColor of
      Just v | not (null v) -> Disabled
      _ -> Enabled
  other -> pure other

resolveGraphemeClusterMode :: Config -> IO GraphemeClusterMode
resolveGraphemeClusterMode c = maybe graphemeClusterModeFromEnv pure (graphemeClusterMode c)

-- | @Configurer::set_history_ignore_dups@
withHistoryIgnoreDups :: Bool -> Config -> Config
withHistoryIgnoreDups yes c =
  c {historyDuplicates = if yes then IgnoreConsecutive else AlwaysAdd}

-- | @Configurer::set_edit_mode@: Vi mode also sets a 500ms key sequence
--   timeout, Emacs mode clears it.
withEditMode :: EditMode -> Config -> Config
withEditMode mode c =
  c
    { editMode = mode,
      keyseqTimeout = case mode of
        Emacs -> Nothing
        Vi -> Just 500
    }
