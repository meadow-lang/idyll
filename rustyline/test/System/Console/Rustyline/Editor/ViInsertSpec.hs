-- | Ported from rustyline's @src/test/vi_insert.rs@.
module System.Console.Rustyline.Editor.ViInsertSpec (spec) where

import System.Console.Rustyline
import System.Console.Rustyline.Editor.Harness
import Test.Hspec

spec :: Spec
spec = do
  it "insert_mode_by_default" $ do
    assertCursor Vi ("", "") [fromChar 'a', ENTER] ("a", "")

  it "ctrl_h" $ do
    assertCursor Vi ("Hi", "") [ctrl 'H', ENTER] ("H", "")

  it "backspace" $ do
    assertCursor Vi ("", "") [BACKSPACE, ENTER] ("", "")
    assertCursor Vi ("Hi", "") [BACKSPACE, ENTER] ("H", "")
    assertCursor Vi ("", "Hi") [BACKSPACE, ENTER] ("", "Hi")

  it "esc" $ do
    assertCursor Vi ("", "") [fromChar 'a', ESC, ENTER] ("", "a")
