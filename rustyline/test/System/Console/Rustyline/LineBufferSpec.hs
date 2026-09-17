module System.Console.Rustyline.LineBufferSpec (spec) where

import Control.Monad.State.Strict (StateT, execStateT, gets, liftIO, runState, state)
import Data.Maybe (fromMaybe)
import Data.Text (Text)
import System.Console.Rustyline.Internal.Text (lenB)
import System.Console.Rustyline.Keymap.Types
import System.Console.Rustyline.Layout
import System.Console.Rustyline.LineBuffer
import Test.Hspec
import Prelude hiding (Word)

-- | Remembers the last deleted string.
newtype Listener = Listener (Maybe Text)

instance DeleteListener Listener where
  onDelete _ string _ _ = Listener (Just string)

instance ChangeListener Listener where
  onInsertChar _ _ = id
  onInsertStr _ _ = id
  onReplace _ _ _ = id

type T = StateT (LineBuffer, Listener) IO

-- | Run an edit on the buffer under test.
lb :: LB Listener a -> T a
lb m = state (runState m)

-- | Run an edit that must not notify anything we check.
withBuffer :: Text -> Int -> T () -> Expectation
withBuffer text p body = () <$ execStateT body (initLineBuffer text p, Listener Nothing)

-- | Reset the buffer, keeping the listener (Rust's @s.buf = ...; s.pos = ...@).
reset :: Text -> Int -> T ()
reset text p = state (\(_, l) -> ((), (initLineBuffer text p, l)))

(==>) :: (Show a, Eq a) => T a -> a -> T ()
m ==> expected = m >>= \actual -> liftIO (actual `shouldBe` expected)

infix 2 ==>

bufIs :: Text -> T ()
bufIs t = gets (asStr . fst) ==> t

posIs :: Int -> T ()
posIs p = gets (pos . fst) ==> p

deleted :: Text -> T ()
deleted t = gets (\(_, Listener d) -> fromMaybe "<nothing deleted>" d) ==> t

query :: (LineBuffer -> a) -> T a
query f = gets (f . fst)

layout0 :: Layout
layout0 = newLayout Unicode

spec :: Spec
spec = do
  it "next_pos" $ do
    let s = initLineBuffer "ö̲g̈" 0
    len s `shouldBe` 7
    nextPos 1 s `shouldBe` Just 4
    nextPos 1 (initLineBuffer "ö̲g̈" 4) `shouldBe` Just 7

  it "prev_pos" $ do
    let s = initLineBuffer "ö̲g̈" 4
    len s `shouldBe` 7
    prevPos 1 s `shouldBe` Just 0
    prevPos 1 (initLineBuffer "ö̲g̈" 7) `shouldBe` Just 4

  it "insert" $ withBuffer "" 0 $ do
    lb (insert 'α' 1) ==> Just True
    bufIs "α"
    posIs 2
    lb (insert 'ß' 1) ==> Just True
    bufIs "αß"
    posIs 4
    lb (setPos 0)
    lb (insert 'γ' 1) ==> Just False
    bufIs "γαß"
    posIs 2

  it "yank_after" $ withBuffer "αß" 2 $ do
    _ <- lb (moveForward 1)
    lb (yank "γδε" 1) ==> Just True
    bufIs "αßγδε"
    posIs 10

  it "yank_before" $ withBuffer "αε" 2 $ do
    lb (yank "ßγδ" 1) ==> Just False
    bufIs "αßγδε"
    posIs 8

  it "moves" $ withBuffer "αß" 4 $ do
    lb (moveBackward 1) ==> True
    bufIs "αß"
    posIs 2
    lb (moveForward 1) ==> True
    posIs 4
    lb moveHome ==> True
    posIs 0
    lb moveEnd ==> True
    bufIs "αß"
    posIs 4

  it "move_home_end_multiline" $ withBuffer "αa\nsdf ßc\nasdf" 7 $ do
    lb moveHome ==> True
    posIs 4
    lb moveHome ==> False
    posIs 4
    lb moveEnd ==> True
    posIs 11
    lb moveEnd ==> False
    posIs 11
    bufIs "αa\nsdf ßc\nasdf"

  it "move_buffer_multiline" $ do
    let text = "αa\nsdf ßc\nasdf"
    withBuffer text 7 $ do
      lb moveBufferStart ==> True
      posIs 0
      lb moveBufferStart ==> False
      posIs 0
      lb moveBufferEnd ==> True
      posIs (lenB text)
      lb moveBufferEnd ==> False
      posIs (lenB text)
      bufIs text

  it "move_grapheme" $ withBuffer "ag̈" 4 $ do
    query len ==> 4
    lb (moveBackward 1) ==> True
    posIs 1
    lb (moveForward 1) ==> True
    posIs 4

  it "delete" $ withBuffer "αß" 2 $ do
    lb (delete 1) ==> Just "ß"
    bufIs "α"
    posIs 2
    lb (backspace 1) ==> True
    bufIs ""
    posIs 0
    deleted "α"

  it "kill" $ withBuffer "αßγδε" 6 $ do
    lb killLine ==> True
    bufIs "αßγ"
    posIs 6
    deleted "δε"
    lb (setPos 4)
    lb discardLine ==> True
    bufIs "γ"
    posIs 0
    deleted "αß"

  it "kill_multiline" $ withBuffer "αß\nγδ 12\nε f4" 7 $ do
    lb killLine ==> True
    bufIs "αß\nγ\nε f4"
    posIs 7
    deleted "δ 12"
    lb killLine ==> True
    bufIs "αß\nγε f4"
    posIs 7
    deleted "\n"
    lb killLine ==> True
    bufIs "αß\nγ"
    posIs 7
    deleted "ε f4"
    lb killLine ==> False
    posIs 7

  it "discard_multiline" $ withBuffer "αß\nc γδε" 9 $ do
    lb discardLine ==> True
    bufIs "αß\nδε"
    posIs 5
    deleted "c γ"
    lb discardLine ==> True
    bufIs "αßδε"
    posIs 4
    deleted "\n"
    lb discardLine ==> True
    bufIs "δε"
    posIs 0
    deleted "αß"
    lb discardLine ==> False
    posIs 0

  it "transpose" $ withBuffer "aßc" 1 $ do
    lb transposeChars ==> True
    bufIs "ßac"
    posIs 3
    reset "aßc" 3
    lb transposeChars ==> True
    bufIs "acß"
    posIs 4
    reset "aßc" 4
    lb transposeChars ==> True
    bufIs "acß"
    posIs 4

  it "move_to_prev_word" $ withBuffer "a ß  c" 6 $ do
    lb (moveToPrevWord WordEmacs 1) ==> True
    bufIs "a ß  c"
    posIs 2
    lb moveEnd ==> True
    posIs 7
    lb (moveToPrevWord WordEmacs 1) ==> True
    posIs 6
    lb (moveToPrevWord WordEmacs 2) ==> True
    posIs 0

  it "move_to_prev_vi_word" $ withBuffer "alpha ,beta/rho; mu" 19 $ do
    mapM_ (\p -> lb (moveToPrevWord WordVi 1) ==> True >> posIs p) [17, 15, 12, 11, 7, 6, 0]
    lb (moveToPrevWord WordVi 1) ==> False

  it "move_to_prev_big_word" $ withBuffer "alpha ,beta/rho; mu" 19 $ do
    mapM_ (\p -> lb (moveToPrevWord WordBig 1) ==> True >> posIs p) [17, 6, 0]
    lb (moveToPrevWord WordBig 1) ==> False

  it "move_to_forward" $ do
    withBuffer "αßγδε" 2 $ lb (moveTo (CSForwardBefore 'ε') 1) ==> True >> posIs 6
    withBuffer "αßγδε" 2 $ lb (moveTo (CSForward 'ε') 1) ==> True >> posIs 8
    withBuffer "αßγδε" 2 $ lb (moveTo (CSForward 'ε') 10) ==> True >> posIs 8

  it "move_to_backward" $ do
    withBuffer "αßγδε" 8 $ lb (moveTo (CSBackwardAfter 'ß') 1) ==> True >> posIs 4
    withBuffer "αßγδε" 8 $ lb (moveTo (CSBackward 'ß') 1) ==> True >> posIs 2

  it "delete_prev_word" $ withBuffer "a ß  c" 6 $ do
    lb (deletePrevWord WordBig 1) ==> True
    bufIs "a c"
    posIs 2
    deleted "ß  "

  it "move_to_next_word" $ withBuffer "a ß  c" 1 $ do
    lb (moveToNextWord AfterEnd WordEmacs 1) ==> True
    bufIs "a ß  c"
    posIs 4
    lb (moveToNextWord AfterEnd WordEmacs 1) ==> True
    posIs 7
    _ <- lb moveHome
    lb (moveToNextWord AfterEnd WordEmacs 1) ==> True
    posIs 1
    lb (moveToNextWord AfterEnd WordEmacs 2) ==> True
    posIs 7

  it "move_to_end_of_word" $ withBuffer "a ßeta  c" 1 $ do
    lb (moveToNextWord BeforeEnd WordVi 1) ==> True
    bufIs "a ßeta  c"
    posIs 6

  it "move_to_end_of_vi_word" $ withBuffer "alpha ,beta/rho; mu" 0 $ do
    mapM_ (\p -> lb (moveToNextWord BeforeEnd WordVi 1) ==> True >> posIs p) [4, 6, 10, 11, 14, 15, 18]
    lb (moveToNextWord BeforeEnd WordVi 1) ==> False

  it "move_to_end_of_big_word" $ withBuffer "alpha ,beta/rho; mu" 0 $ do
    mapM_ (\p -> lb (moveToNextWord BeforeEnd WordBig 1) ==> True >> posIs p) [4, 15, 18]
    lb (moveToNextWord BeforeEnd WordBig 1) ==> False

  it "move_to_start_of_word" $ withBuffer "a ß  c" 2 $ do
    lb (moveToNextWord Start WordEmacs 1) ==> True
    bufIs "a ß  c"
    posIs 6

  it "move_to_start_of_vi_word" $ withBuffer "alpha ,beta/rho; mu" 0 $ do
    mapM_ (\p -> lb (moveToNextWord Start WordVi 1) ==> True >> posIs p) [6, 7, 11, 12, 15, 17, 18]
    lb (moveToNextWord Start WordVi 1) ==> False

  it "move_to_start_of_big_word" $ withBuffer "alpha ,beta/rho; mu" 0 $ do
    mapM_ (\p -> lb (moveToNextWord Start WordBig 1) ==> True >> posIs p) [6, 17, 18]
    lb (moveToNextWord Start WordBig 1) ==> False

  it "delete_word" $ do
    withBuffer "a ß  c" 1 $ do
      lb (deleteWord AfterEnd WordEmacs 1) ==> True
      bufIs "a  c"
      posIs 1
      deleted " ß"
    withBuffer "test" 0 $ do
      lb (deleteWord AfterEnd WordVi 1) ==> True
      bufIs ""
      posIs 0
      deleted "test"

  it "delete_til_start_of_word" $ withBuffer "a ß  c" 2 $ do
    lb (deleteWord Start WordEmacs 1) ==> True
    bufIs "a c"
    posIs 2
    deleted "ß  "

  it "delete_to_forward" $ do
    withBuffer "αßγδε" 2 $ do
      lb (deleteTo (CSForwardBefore 'ε') 1) ==> True
      deleted "ßγδ"
      bufIs "αε"
      posIs 2
    withBuffer "αßγδε" 2 $ do
      lb (deleteTo (CSForward 'ε') 1) ==> True
      deleted "ßγδε"
      bufIs "α"
      posIs 2

  it "delete_to_backward" $ do
    withBuffer "αßγδε" 8 $ do
      lb (deleteTo (CSBackwardAfter 'α') 1) ==> True
      deleted "ßγδ"
      bufIs "αε"
      posIs 2
    withBuffer "αßγδε" 8 $ do
      lb (deleteTo (CSBackward 'ß') 1) ==> True
      deleted "ßγδ"
      bufIs "αε"
      posIs 2

  it "edit_word" $ do
    withBuffer "a ßeta  c" 1 $ lb (editWord Uppercase) ==> True >> bufIs "a SSETA  c" >> posIs 7
    withBuffer "a ßetA  c" 1 $ lb (editWord Lowercase) ==> True >> bufIs "a ßeta  c" >> posIs 7
    withBuffer "a ßETA  c" 1 $ lb (editWord Capitalize) ==> True >> bufIs "a SSeta  c" >> posIs 7
    withBuffer "test" 1 $ lb (editWord Capitalize) ==> True >> bufIs "tEst" >> posIs 4

  it "transpose_words" $ do
    withBuffer "ßeta / δelta__" 15 $ lb (transposeWords 1) ==> True >> bufIs "δelta__ / ßeta" >> posIs 16
    withBuffer "ßeta / δelta" 14 $ lb (transposeWords 1) ==> True >> bufIs "δelta / ßeta" >> posIs 14
    withBuffer " / δelta" 8 $ lb (transposeWords 1) ==> False
    withBuffer "ßeta / __" 9 $ lb (transposeWords 1) ==> False

  it "move_by_line" $ withBuffer "aa123\nsdf bc\nasdf" 14 $ do
    let up n l = lb (moveToLineUp n l)
        down n l = lb (moveToLineDown n l)
    up 1 layout0 ==> True >> posIs 7
    up 1 layout0 ==> True >> posIs 1
    up 1 layout0 ==> False >> posIs 1
    down 1 layout0 ==> True >> posIs 7
    down 1 layout0 ==> True >> posIs 14
    down 1 layout0 ==> False >> posIs 14
    up 2 layout0 ==> True >> posIs 1
    down 2 layout0 ==> True >> posIs 14
    -- non-empty prompt
    let layout2 = layout0 {layoutPromptSize = (layoutPromptSize layout0) {posCol = 2}}
    _ <- up 1 layout2
    posIs 7
    _ <- up 1 layout2
    posIs 0
    _ <- down 1 layout2
    posIs 8
