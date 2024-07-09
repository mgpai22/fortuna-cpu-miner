# Fortuna CPU Miner

Multithreaded rust CPU miner to mine fortuna on the cardano blockchain. This is only compatible with a specific stratum
pool designed for fortuna atm.

# EXE Download Instructions

- Download the [latest release](https://github.com/mgpai22/fortuna-cpu-miner/releases/) for your platform
- Unzip downloaded folder
- Edit mine.sh or mine.bat depending on your platform
- run: `./mine.sh`

## Mine Script Instructions

- Add the specified pool url
- Follow the format of `<address>.<name>` for `USER` to get rewarded properly
- Choose thread count, do most threads if its a dedicated mining rig, do half or quarter available threads if you want
  to use the computer while you mine. The higher the thread count, the higher your hashrate
- By default, miner uses pool's base difficulty. This can be problematic if your miner finds solutions fast. You may
  need to manually specify difficulty as such
    - `./mine.sh --difficulty <num>`

# Build Instructions

- `cargo build --release`
- you can find the binary in releases directory
