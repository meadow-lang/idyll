module Main (main) where

import Control.Exception (IOException, try)
import Control.Monad (unless, void)
import Data.Text (Text)
import Data.Text qualified as T
import GHC.IO.Handle (BufferMode (..), hSetBuffering)
import Idyllic.Pipeline
import System.Console.Rustyline
import System.IO (stdout)

historyPath :: FilePath
historyPath = ".repl_history"

banner :: String
banner =
  """
      ____    __      ____
     /  _/___/ /_  __/ / /
     / // __  / / / / / / 
   _/ // /_/ / /_/ / / /  
  /___/\\__,_/\\__, /_/_/   
            /____/        
  Welcome to the Idyll REPL!
  Type :quit or :q to exit.
  """

-- | An entry is submitted by a blank line. REPL commands (a single line
--   starting with ':') and an empty buffer are submitted right away.
blankLineValidator :: Validator
blankLineValidator =
  defaultValidator
    { validate = \ctx ->
        let input = validationInput ctx
            lastLine = T.takeWhileEnd (/= '\n') input
         in pure $
              if
                | T.null (T.strip input) -> Valid Nothing
                | isCommand input -> Valid Nothing
                | T.null (T.strip lastLine) -> Valid Nothing
                | otherwise -> Incomplete
    }

isCommand :: Text -> Bool
isCommand input = ":" `T.isPrefixOf` T.stripStart input && not (T.any (== '\n') (T.strip input))

main :: IO ()
main = do
  hSetBuffering stdout NoBuffering
  putStrLn banner
  rl <- newEditor defaultConfig {maxHistorySize = 1000}
  setHelper rl (Just defaultHelper {helperValidator = blankLineValidator})
  -- a missing history file is fine
  void (try @IOException (loadHistory rl historyPath))
  loop rl
  where
    loop rl =
      readline rl "> " >>= \case
        Left Interrupted -> loop rl -- Ctrl-C discards the entry
        Left Eof -> pure ()
        Left err -> putStrLn ("readline error: " <> show err)
        Right raw -> do
          let entry = T.stripEnd raw
          unless (T.null (T.strip entry)) $ do
            _ <- addHistoryEntry rl entry
            void (try @IOException (appendHistory rl historyPath))
          case T.strip entry of
            cmd | cmd == ":quit" || cmd == ":q" -> pure ()
            "" -> loop rl
            _ -> do
              out <- runPipelineIO False InputModeInteractive (entry <> "\n")
              putStrLn out
              loop rl
