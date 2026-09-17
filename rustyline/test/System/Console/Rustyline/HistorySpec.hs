module System.Console.Rustyline.HistorySpec (spec) where

import Control.Exception (bracket)
import Data.ByteString qualified as BS
import Data.Text (Text)
import System.Console.Rustyline.Config
import System.Console.Rustyline.History
import System.Directory (getTemporaryDirectory, removeFile)
import System.IO (hClose, openTempFile)
import Test.Hspec

initHistory :: IO DefaultHistory
initHistory = do
  h <- newFileHistory
  mapM_ (\l -> historyAdd h l `shouldReturn` True) ["line1", "line2", "line3"]
  pure h

withTempFile :: (FilePath -> IO a) -> IO a
withTempFile = bracket create removeFile
  where
    create = do
      dir <- getTemporaryDirectory
      (path, h) <- openTempFile dir "rustyline-history"
      hClose h
      pure path

checkSave :: Text -> Expectation
checkSave line = do
  history <- initHistory
  historyAdd history line `shouldReturn` True
  withTempFile $ \tf -> do
    historySave history tf
    history2 <- newFileHistory
    historyLoad history2 tf
    expected <- historyEntries history
    historyEntries history2 `shouldReturn` expected

entry :: DefaultHistory -> Int -> IO Text
entry h i = (!! i) <$> historyEntries h

found :: DefaultHistory -> Int -> Int -> IO (Maybe SearchResult)
found h idx p = do
  e <- historyGet h idx SearchForward
  pure ((\r -> r {srPos = p}) <$> e)

spec :: Spec
spec = do
  it "new" $ do
    h <- newFileHistory
    historyLen h `shouldReturn` 0

  it "add" $ do
    h <- fileHistoryWithConfig defaultConfig {historyIgnoreSpace = True}
    historyAdd h "line1" `shouldReturn` True
    historyAdd h "line2" `shouldReturn` True
    historyAdd h "line2" `shouldReturn` False
    historyAdd h "" `shouldReturn` False
    historyAdd h " line3" `shouldReturn` False

  it "set_max_len" $ do
    h <- initHistory
    historySetMaxLen h 1
    historyLen h `shouldReturn` 1
    historyEntries h `shouldReturn` ["line3"]

  it "save" $ checkSave "line\nfour \\ abc"

  it "save_windows_path" $ checkSave "cd source\\repos\\forks\\nushell\\"

  it "load_legacy" $ withTempFile $ \tf -> do
    BS.writeFile tf "test\\n \\abc \\123\n123\\n\\\\n\nabcde\n            "
    h <- newFileHistory
    historyLoad h tf
    entry h 0 `shouldReturn` "test\\n \\abc \\123"
    entry h 1 `shouldReturn` "123\\n\\\\n"
    entry h 2 `shouldReturn` "abcde"

  it "append" $ withTempFile $ \tf -> do
    history <- initHistory
    historyAppend history tf
    history2 <- newFileHistory
    historyLoad history2 tf
    _ <- historyAdd history2 "line4"
    historyAppend history2 tf
    _ <- historyAdd history "line5"
    historyAppend history tf
    history3 <- newFileHistory
    historyLoad history3 tf
    historyLen history3 `shouldReturn` 5

  it "truncate" $ withTempFile $ \tf -> do
    h1 <- fileHistoryWithConfig (withHistoryIgnoreDups False defaultConfig)
    _ <- historyAdd h1 "line1"
    _ <- historyAdd h1 "line1"
    historyAppend h1 tf
    h2 <- newFileHistory
    historyLoad h2 tf
    _ <- historyAdd h2 "l"
    historyAppend h2 tf
    h3 <- newFileHistory
    historyLoad h3 tf
    historyLen h3 `shouldReturn` 2
    entry h3 1 `shouldReturn` "l"

  it "search" $ do
    h <- initHistory
    historySearch h "" 0 SearchForward `shouldReturn` Nothing
    historySearch h "none" 0 SearchForward `shouldReturn` Nothing
    historySearch h "line" 3 SearchForward `shouldReturn` Nothing
    (historySearch h "line" 0 SearchForward >>=) . shouldBe =<< found h 0 0
    (historySearch h "line" 1 SearchForward >>=) . shouldBe =<< found h 1 0
    (historySearch h "line3" 1 SearchForward >>=) . shouldBe =<< found h 2 0

  it "reverse_search" $ do
    h <- initHistory
    historySearch h "" 2 SearchReverse `shouldReturn` Nothing
    historySearch h "none" 2 SearchReverse `shouldReturn` Nothing
    historySearch h "line" 3 SearchReverse `shouldReturn` Nothing
    (historySearch h "line" 2 SearchReverse >>=) . shouldBe =<< found h 2 0
    (historySearch h "line" 1 SearchReverse >>=) . shouldBe =<< found h 1 0
    (historySearch h "line1" 1 SearchReverse >>=) . shouldBe =<< found h 0 0

  it "writes a #V2 file readable by rustyline" $ withTempFile $ \tf -> do
    h <- newFileHistory
    _ <- historyAdd h "a\nb\\c"
    historySave h tf
    BS.readFile tf `shouldReturn` "#V2\na\\nb\\\\c\n"
