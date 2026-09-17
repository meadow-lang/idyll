-- | Ported from rustyline's @src/test/history.rs@.
module System.Console.Rustyline.Editor.HistorySpec (spec) where

import Control.Monad (forM_)
import System.Console.Rustyline
import System.Console.Rustyline.Editor.Harness
import Test.Hspec

spec :: Spec
spec = do
  it "down_key" $ do
    forM_ [Emacs, Vi] $ \mode -> do
      assertHistory mode ["line1"] [KeyEvent KDown NONE, ENTER] "" ("", "")
      assertHistory mode ["line1", "line2"] [KeyEvent KUp NONE, KeyEvent KUp NONE, KeyEvent KDown NONE, ENTER] "" ("line2", "")
      assertHistory mode ["line1"] [fromChar 'a', KeyEvent KUp NONE, KeyEvent KDown NONE, ENTER] "" ("a", "")
      assertHistory mode ["line1"] [fromChar 'a', KeyEvent KDown NONE, ENTER] "" ("a", "")

  it "up_key" $ do
    forM_ [Emacs, Vi] $ \mode -> do
      assertHistory mode [] [KeyEvent KUp NONE, ENTER] "" ("", "")
      assertHistory mode ["line1"] [KeyEvent KUp NONE, ENTER] "" ("line1", "")
      assertHistory mode ["line1", "line2"] [KeyEvent KUp NONE, KeyEvent KUp NONE, ENTER] "" ("line1", "")

  it "ctrl_r" $ do
    forM_ [Emacs, Vi] $ \mode -> do
      assertHistory mode [] [ctrl 'R', fromChar 'o', ENTER] "" ("o", "")
      assertHistory mode ["rustc", "cargo"] [ctrl 'R', fromChar 'o', KeyEvent KRight NONE, ENTER] "" ("cargo", "")
      assertHistory mode ["rustc", "cargo"] [ctrl 'R', fromChar 'u', KeyEvent KRight NONE, ENTER] "" ("ru", "stc")
      assertHistory mode ["rustc", "cargo"] [ctrl 'R', fromChar 'r', fromChar 'u', KeyEvent KRight NONE, ENTER] "" ("r", "ustc")
      assertHistory mode ["rustc", "cargo"] [ctrl 'R', fromChar 'r', ctrl 'R', KeyEvent KRight NONE, ENTER] "" ("r", "ustc")
      assertHistory mode ["rustc", "cargo"] [ctrl 'R', fromChar 'r', fromChar 'z', KeyEvent KRight NONE, ENTER] "" ("car", "go")
      assertHistory Emacs ["rustc", "cargo"] [fromChar 'a', ctrl 'R', fromChar 'r', ctrl 'G', ENTER] "" ("a", "")

  it "ctrl_r_with_long_prompt" $ do
    forM_ [Emacs, Vi] $ \mode -> do
      assertHistory mode ["rustc", "cargo"] [ctrl 'R', fromChar 'o', ENTER] ">>>>>>>>>>>>>>>>>>>>>>>>>>> " ("cargo", "")

  it "ctrl_s" $ do
    forM_ [Emacs, Vi] $ \mode -> do
      assertHistory mode ["rustc", "cargo"] [ctrl 'R', fromChar 'r', ctrl 'R', ctrl 'S', KeyEvent KRight NONE, ENTER] "" ("car", "go")

  it "meta_lt" $ do
    assertHistory Emacs [""] [alt '<', ENTER] "" ("", "")
    assertHistory Emacs ["rustc", "cargo"] [alt '<', ENTER] "" ("rustc", "")

  it "meta_gt" $ do
    assertHistory Emacs [""] [alt '>', ENTER] "" ("", "")
    assertHistory Emacs ["rustc", "cargo"] [alt '<', alt '>', ENTER] "" ("", "")
    assertHistory Emacs ["rustc", "cargo"] [fromChar 'a', alt '<', alt '>', ENTER] "" ("a", "")
