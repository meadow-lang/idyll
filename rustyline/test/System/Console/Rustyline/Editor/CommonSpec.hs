-- | Ported from rustyline's @src/test/common.rs@.
module System.Console.Rustyline.Editor.CommonSpec (spec) where

import Control.Monad (forM_, when)
import System.Console.Rustyline
import System.Console.Rustyline.Editor.Harness
import Test.Hspec

spec :: Spec
spec = do
  it "home_key" $ do
    forM_ [Emacs, Vi] $ \mode -> do
      assertCursor mode ("", "") [KeyEvent KHome NONE, ENTER] ("", "")
      assertCursor mode ("Hi", "") [KeyEvent KHome NONE, ENTER] ("", "Hi")
      when (mode == Vi) $ do
        -- vi command mode
        assertCursor mode ("Hi", "") [ESC, KeyEvent KHome NONE, ENTER] ("", "Hi")

  it "end_key" $ do
    forM_ [Emacs, Vi] $ \mode -> do
      assertCursor mode ("", "") [KeyEvent KEnd NONE, ENTER] ("", "")
      assertCursor mode ("H", "i") [KeyEvent KEnd NONE, ENTER] ("Hi", "")
      assertCursor mode ("", "Hi") [KeyEvent KEnd NONE, ENTER] ("Hi", "")
      when (mode == Vi) $ do
        -- vi command mode
        assertCursor mode ("", "Hi") [ESC, KeyEvent KEnd NONE, ENTER] ("Hi", "")

  it "left_key" $ do
    forM_ [Emacs, Vi] $ \mode -> do
      assertCursor mode ("Hi", "") [KeyEvent KLeft NONE, ENTER] ("H", "i")
      assertCursor mode ("H", "i") [KeyEvent KLeft NONE, ENTER] ("", "Hi")
      assertCursor mode ("", "Hi") [KeyEvent KLeft NONE, ENTER] ("", "Hi")
      when (mode == Vi) $ do
        -- vi command mode
        assertCursor mode ("Bye", "") [ESC, KeyEvent KLeft NONE, ENTER] ("B", "ye")

  it "right_key" $ do
    forM_ [Emacs, Vi] $ \mode -> do
      assertCursor mode ("", "") [KeyEvent KRight NONE, ENTER] ("", "")
      assertCursor mode ("", "Hi") [KeyEvent KRight NONE, ENTER] ("H", "i")
      assertCursor mode ("B", "ye") [KeyEvent KRight NONE, ENTER] ("By", "e")
      assertCursor mode ("H", "i") [KeyEvent KRight NONE, ENTER] ("Hi", "")
      when (mode == Vi) $ do
        -- vi command mode
        assertCursor mode ("", "Hi") [ESC, KeyEvent KRight NONE, ENTER] ("H", "i")

  it "enter_key" $ do
    forM_ [Emacs, Vi] $ \mode -> do
      assertLine mode [ENTER] ""
      assertLine mode [fromChar 'a', ENTER] "a"
      assertLineWithInitial mode ("Hi", "") [ENTER] "Hi"
      assertLineWithInitial mode ("", "Hi") [ENTER] "Hi"
      assertLineWithInitial mode ("H", "i") [ENTER] "Hi"
      when (mode == Vi) $ do
        -- vi command mode
        assertLine mode [ESC, ENTER] ""
        assertLine mode [fromChar 'a', ESC, ENTER] "a"
        assertLineWithInitial mode ("Hi", "") [ESC, ENTER] "Hi"
        assertLineWithInitial mode ("", "Hi") [ESC, ENTER] "Hi"
        assertLineWithInitial mode ("H", "i") [ESC, ENTER] "Hi"

  it "newline_key" $ do
    forM_ [Emacs, Vi] $ \mode -> do
      assertLine mode [ctrl 'J'] ""
      assertLine mode [fromChar 'a', ctrl 'J'] "a"
      when (mode == Vi) $ do
        -- vi command mode
        assertLine mode [ESC, ctrl 'J'] ""
        assertLine mode [fromChar 'a', ESC, ctrl 'J'] "a"

  it "eof_key" $ do
    forM_ [Emacs, Vi] $ \mode -> do
      assertError mode [ctrl 'D'] Nothing Eof
    assertLine Emacs [fromChar 'a', ctrl 'D', ENTER] "a"
    assertLine Vi [fromChar 'a', ctrl 'D'] "a"
    assertLine Vi [fromChar 'a', ESC, ctrl 'D'] "a"
    assertLineWithInitial Emacs ("", "Hi") [ctrl 'D', ENTER] "i"
    assertLineWithInitial Vi ("", "Hi") [ctrl 'D'] "Hi"
    assertLineWithInitial Vi ("", "Hi") [ESC, ctrl 'D'] "Hi"

  it "interrupt_key" $ do
    forM_ [Emacs, Vi] $ \mode -> do
      assertError mode [ctrl 'C'] Nothing Interrupted
      assertError mode [ctrl 'C'] (Just ("Hi", "")) Interrupted
      when (mode == Vi) $ do
        -- vi command mode
        assertError mode [ESC, ctrl 'C'] (Just ("Hi", "")) Interrupted

  it "delete_key" $ do
    forM_ [Emacs, Vi] $ \mode -> do
      assertCursor mode ("a", "") [KeyEvent KDelete NONE, ENTER] ("a", "")
      assertCursor mode ("", "a") [KeyEvent KDelete NONE, ENTER] ("", "")
      when (mode == Vi) $ do
        -- vi command mode
        assertCursor mode ("", "a") [ESC, KeyEvent KDelete NONE, ENTER] ("", "")

  it "ctrl_t" $ do
    forM_ [Emacs, Vi] $ \mode -> do
      assertCursor mode ("a", "b") [ctrl 'T', ENTER] ("ba", "")
      assertCursor mode ("ab", "cd") [ctrl 'T', ENTER] ("acb", "d")
      when (mode == Vi) $ do
        -- vi command mode
        assertCursor mode ("ab", "") [ESC, ctrl 'T', ENTER] ("ba", "")

  it "ctrl_u" $ do
    forM_ [Emacs, Vi] $ \mode -> do
      assertCursor mode ("start of line ", "end") [ctrl 'U', ENTER] ("", "end")
      assertCursor mode ("", "end") [ctrl 'U', ENTER] ("", "end")
      when (mode == Vi) $ do
        -- vi command mode
        assertCursor mode ("start of line ", "end") [ESC, ctrl 'U', ENTER] ("", " end")

  it "ctrl_v" $ do
    forM_ [Emacs, Vi] $ \mode -> do
      assertCursor mode ("", "") [ctrl 'V', KeyEvent (KChar '\t') NONE, ENTER] ("\t", "")
      when (mode == Vi) $ do
        -- vi command mode
        assertCursor mode ("", "") [ESC, ctrl 'V', KeyEvent (KChar '\t') NONE, ENTER] ("\t", "")

  it "ctrl_w" $ do
    forM_ [Emacs, Vi] $ \mode -> do
      assertCursor mode ("Hello, ", "world") [ctrl 'W', ENTER] ("", "world")
      assertCursor mode ("Hello, world.", "") [ctrl 'W', ENTER] ("Hello, ", "")
      when (mode == Vi) $ do
        -- vi command mode
        assertCursor mode ("Hello, world.", "") [ESC, ctrl 'W', ENTER] ("Hello, ", ".")

  it "ctrl_y" $ do
    forM_ [Emacs] $ \mode -> do
      assertCursor mode ("Hello, ", "world") [ctrl 'W', ctrl 'Y', ENTER] ("Hello, ", "world")

  it "ctrl__" $ do
    forM_ [Emacs, Vi] $ \mode -> do
      assertCursor mode ("Hello, ", "world") [ctrl 'W', ctrl '_', ENTER] ("Hello, ", "world")
      when (mode == Vi) $ do
        -- vi command mode
        assertCursor mode ("Hello, ", "world") [ESC, ctrl 'W', ctrl '_', ENTER] ("Hello,", " world")

  it "paste" $ do
    forM_ [Emacs, Vi] $ \mode -> do
      assertCursor mode ("", "") [KeyEvent KBracketedPasteStart NONE, ENTER] ("pasted", "")
      when (mode == Vi) $ do
        -- vi command mode
        assertCursor mode ("", "") [ESC, KeyEvent KBracketedPasteStart NONE, ENTER] ("paste", "d")
