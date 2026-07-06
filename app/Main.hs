module Main (main) where

import Control.Monad.IO.Class (liftIO)
import Data.Text (pack)
import GHC.IO.Handle (BufferMode (..), hSetBuffering)
import Idyllic.Effect
import Idyllic.Pipeline (runPipelineIO)
import System.Console.Haskeline
import System.IO (stdout)

mySettings :: Settings IO
mySettings =
  defaultSettings
    { historyFile = Just ".repl_history",
      autoAddHistory = True
    }

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

main :: IO ()
main = do
  hSetBuffering stdout NoBuffering
  putStrLn banner
  runInputT mySettings (loop [])
  where
    loop :: [String] -> InputT IO ()
    loop acc = do
      let prompt = if null acc then "> " else "| "
      minput <- getInputLine prompt
      case minput of
        Nothing -> return ()
        Just line
          | line == ":quit" || line == ":q" -> return ()
          | line == ":reset" -> do
              liftIO $ putStrLn "Resetting REPL state..."
              loop []
        Just line
          | null line && not (null acc) -> do
              -- blank line and we have accumulated input: process it
              let code = unlines (reverse acc)
              liftIO (runOneInput code)
              loop []
          | null line -> loop acc -- blank line with nothing typed yet, ignore
          | otherwise -> loop (line : acc)

runOneInput :: String -> IO ()
runOneInput code = runPipelineIO False InputModeInteractive (pack code)