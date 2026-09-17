-- | Test helpers from rustyline's @src/test/mod.rs@.
module System.Console.Rustyline.Editor.Harness
  ( initEditor,
    assertLine,
    assertLineWithInitial,
    assertCursor,
    assertHistory,
    assertError,
  )
where

import Control.Monad (forM_, when)
import Data.IORef
import Data.Text (Text)
import Data.Text qualified as T
import System.Console.Rustyline
import System.Console.Rustyline.Internal.Text (lenB)
import System.Console.Rustyline.Tty.Test
import Test.Hspec

initEditor :: EditMode -> [KeyEvent] -> IO Editor
initEditor mode keys = do
  let config = withEditMode mode defaultConfig
  history <- fileHistoryWithConfig config
  dt <- newDummyTerminal
  writeIORef (dummyKeys dt) keys
  newEditorWithTerminal config history (dummyTerminal dt)

-- | The line after the keys are pressed.
assertLine :: EditMode -> [KeyEvent] -> Text -> Expectation
assertLine mode keys expected = do
  editor <- initEditor mode keys
  readline editor ">>" `shouldReturnLine` expected

-- | The line after the keys are pressed on the initial line (text before
--   and after the cursor).
assertLineWithInitial :: EditMode -> (Text, Text) -> [KeyEvent] -> Text -> Expectation
assertLineWithInitial mode initial keys expected = do
  editor <- initEditor mode keys
  readlineWithInitial editor ">>" initial `shouldReturnLine` expected

-- | The line and cursor before the enter key.
assertCursor :: EditMode -> (Text, Text) -> [KeyEvent] -> (Text, Text) -> Expectation
assertCursor mode initial keys (before, after) = do
  editor <- initEditor mode keys
  readlineWithInitial editor "" initial `shouldReturnLine` (before <> after)
  lastAcceptCursor editor `shouldReturn` lenB before

-- | Like 'assertCursor', starting from history entries.
assertHistory :: EditMode -> [Text] -> [KeyEvent] -> Text -> (Text, Text) -> Expectation
assertHistory mode entries keys prompt' (before, after) = do
  editor <- initEditor mode keys
  forM_ entries (addHistoryEntry editor)
  readline editor (prompt prompt') `shouldReturnLine` (before <> after)
  when (T.null prompt') $ lastAcceptCursor editor `shouldReturn` lenB before

-- | The keys end reading with the error.
assertError :: EditMode -> [KeyEvent] -> Maybe (Text, Text) -> ReadlineError -> Expectation
assertError mode keys initial expected = do
  editor <- initEditor mode keys
  r <- maybe (readline editor ">>") (readlineWithInitial editor ">>") initial
  either show (("Right " <>) . show) r `shouldBe` show expected

shouldReturnLine :: IO (Either ReadlineError Text) -> Text -> Expectation
shouldReturnLine action expected =
  action >>= \case
    Right line -> line `shouldBe` expected
    Left e -> expectationFailure ("readline failed: " <> show e)
