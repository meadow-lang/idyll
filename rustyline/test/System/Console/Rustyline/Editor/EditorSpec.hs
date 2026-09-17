-- | Ported from rustyline's @src/test/mod.rs@ and the @edit.rs@ tests.
module System.Console.Rustyline.Editor.EditorSpec (spec) where

import Control.Monad (forM_)
import Data.IORef
import Data.Text (Text)
import System.Console.Rustyline
import System.Console.Rustyline.Edit (initState, readLine, stHistoryIndex, stSavedLine, editHistoryNext)
import System.Console.Rustyline.Editor.Harness
import System.Console.Rustyline.Keymap (newInputState)
import System.Console.Rustyline.LineBuffer (asStr, pos)
import System.Console.Rustyline.Tty.Test
import Test.Hspec

simpleCompleter :: Helper
simpleCompleter =
  defaultHelper
    { helperCompleter =
        defaultCompleter
          { complete = \line _ _ ->
              pure
                ( 0,
                  map candidate $
                    if
                      | line == "rus" -> [line <> "t"]
                      | line == "\\hbar" -> ["ℏ"]
                      | otherwise -> []
                )
          }
    }

runCompleteLine :: Text -> Int -> Config -> IO (Maybe Cmd, Text, Int)
runCompleteLine line p config = do
  history <- newFileHistory
  s <- initState sink line p (Just simpleCompleter) (AnyHistory history)
  bindings <- newIORef emptyBindings
  is <- newInputState config bindings True
  rdr <- keysReader [ENTER]
  cmd <- completeLine rdr s is config
  lb <- readLine s
  pure (cmd, asStr lb, pos lb)

spec :: Spec
spec = do
  it "complete_line" $
    runCompleteLine "rus" 3 defaultConfig
      `shouldReturn` (Just (AcceptOrInsertLine True), "rust", 4)

  it "complete_symbol" $
    runCompleteLine "\\hbar" 5 defaultConfig {completionType = List}
      `shouldReturn` (Nothing, "ℏ", 3)

  it "unknown_esc_key" $
    forM_ [Emacs, Vi] $ \mode ->
      assertLine mode [KeyEvent KUnknownEscSeq NONE, ENTER] ""

  it "test_apply_backspace_direct" $
    applyBackspaceDirect "Hel\b\bel\bllo ☹\b☺" `shouldBe` "Hello ☺"

  it "test_readline_direct" $ do
    inputs <- newIORef ["([)\n", "\b\n", "\n", "\r\n", "])"]
    written <- newIORef []
    let next = atomicModifyIORef' inputs $ \case
          (l : rest) -> (rest, Just l)
          [] -> ([], Nothing)
    output <- readlineDirectWith next (\t -> modifyIORef' written (t :)) (Just matchingBracketValidator)
    readIORef written `shouldReturn` ["Mismatched brackets: '[' is not properly closed"]
    output `shouldBe` "([\n\n\r\n])"

  it "edit_history_next" $ do
    history <- newFileHistory
    _ <- historyAdd history "line0"
    _ <- historyAdd history "line1"
    let line = "current edited line"
    s <- initState sink line 6 Nothing (AnyHistory history)
    let check saved idx current = do
          maybe (pure ()) (\t -> (asStr <$> readIORef (stSavedLine s)) `shouldReturn` t) saved
          readIORef (stHistoryIndex s) `shouldReturn` idx
          (asStr <$> readLine s) `shouldReturn` current
    forM_ [1 :: Int, 2] $ \_ -> do
      editHistoryNext s False
      (asStr <$> readLine s) `shouldReturn` line
    editHistoryNext s True
    check (Just line) 1 "line1"
    forM_ [1 :: Int, 2] $ \_ -> do
      editHistoryNext s True
      check (Just line) 0 "line0"
    editHistoryNext s False
    check (Just line) 1 "line1"
    editHistoryNext s False
    check Nothing 2 line
