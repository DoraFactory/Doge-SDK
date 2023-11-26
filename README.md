# Doge-SDK
Cosmos Rust SDK for Dear Doge

## Interview
Doge SDK is mainly used to start a cosmos-sdk-based blockchain in rust. Due to the fact that Cosmos SDK currently only has Go language version, we have implemented a Rust version of the Cosmos SDK to meet the requirements for different consensus mechanisms(the Go version of the Cosmos SDK is tightly coupled with the tendermint consensus, we have also implemented a separate SDK for starting the application, which is decoupled from the consensus layer). This allows us to test Rust-based consensus mechanisms more effectively.

### Architecture
The following is the general architecture of our designed implementation of Doge SDK.

![image](https://hackmd.io/_uploads/rySDd-BNp.png)


### Requirements

Doge SDK uses the [tendermint-abci](https://crates.io/crates/tendermint-abci) to communicate between Asynchronous Consensus instance and app, which run as separate process.

- Rust compiler

The minimum supported Rust version is `1.67.1`. Follow the [installation instructions](https://doc.rust-lang.org/book/ch01-01-installation.html).


- libclang
```shell
sudo apt-get install libclang-dev build-essential
```

### Running a local chain

1. Clone this repo:

```console
git clone https://github.com/DoraFactory/Doge-SDK.git
cd Doge-SDK
```

2. Initialize a SDK client:


```console
make init
```

3. Build and start the application:

```console
make run
```

The application will listen for connections on `127.0.0.1:26658`.

4. From a different terminal window start Asynchronous Consensus:

```console
git clone https://github.com/DoraFactory/consensus-abci.git && cd consensus-abci

cargo build --release && ./target/release/hbbft run --abci-server 26668 --rpc-port 26667
```

Asynchronous Consensus will connect to the application and bind it's RPC server to `127.0.0.1:26657`.

The chain (consisting of one node) is now up and running.


### How to use Dear SDK

In this section we'll install Ddoged and use it to query the chain (just like cosmos-sdk based chains the Ddoged binary serves as a node and client).

>By default, the executable file we compile is named `Ddogd` and using cosmos address

1. Install Ddoged:

```console
make install
```

2. Query a balance:    

```console
Ddoged query bank balances cosmos1syavy2npfyt9tcncdtsdzf7kny9lh777pahuux
```

which returns:

```json
{
  "balances": [
    {
      "denom": "uatom",
      "amount": "34"
    }
  ],
  "pagination": null
}
```

The balance of this address was set to 34 in the genesis file.

3. Import the key corresponding to the above address into the gaia-rs key store:

```console
echo "race draft rival universe maid cheese steel logic crowd fork comic easy truth drift tomorrow eye buddy head time cash swing swift midnight borrow" | Ddoged keys add alice
```

4. Send tokens:

```console
Ddoged tx bank send alice cosmos180tr8wmsk8ugt32yynj8efqwg3yglmpwp22rut 10uatom --fee 1uatom
```

5. Query the address balance and observe that it has decreased by 11uatom which is the sum of the amount transferred and the fee:

```console
Ddoged query bank balances cosmos1syavy2npfyt9tcncdtsdzf7kny9lh777pahuux
```

Which returns:

```json
{
  "balances": [
    {
      "denom": "uatom",
      "amount": "23"
    }
  ],
  "pagination": null
}

```

### Resources
Now we do not provide the public rpc and rest api for using. In the near future, we will gradually open it up for testing.

