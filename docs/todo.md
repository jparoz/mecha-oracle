# UI issues

# Gameplay issues
- On each main phase of each player's turn, the player is still allowed to play lands/creatures/etc. after they've passed priority. This is both a UI issue (the options shouldn't be visible after passing priority), and a server rules issue (the commands to play a land or cast a spell should be forbidden). The server needs to check who has priority before playing lands/casting spells.
- I can still use spacebar to advance the steps after the game has finished; this should be forbidden.
