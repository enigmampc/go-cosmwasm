package types

//---------- Env ---------

// Env defines the state of the blockchain environment this contract is
// running in. This must contain only trusted data - nothing from the Tx itself
// that has not been verfied (like Signer).
//
// Env are json encoded to a byte slice before passing to the wasm contract.
type Env struct {
	Block    BlockInfo    `json:"block"`
	Message  MessageInfo  `json:"message"`
	Contract ContractInfo `json:"contract"`
	Key      ContractKey  `json:"contract_key"`
}

type ContractKey string

type BlockInfo struct {
	// block height this transaction is executed
	Height int64 `json:"height"`
	// time in seconds since unix epoch - since cosmwasm 0.3
	Time    int64  `json:"time"`
	ChainID string `json:"chain_id"`
}

type MessageInfo struct {
	// binary encoding of sdk.AccAddress executing the contract
	Signer CanonicalAddress `json:"signer"`
	// amount of funds send to the contract along with this message
	SentFunds []Coin `json:"sent_funds"`
}

type ContractInfo struct {
	// binary encoding of sdk.AccAddress of the contract, to be used when sending messages
	Address CanonicalAddress `json:"address"`
	// current balance of the account controlled by the contract
	Balance []Coin `json:"balance"`
}

// Coin is a string representation of the sdk.Coin type (more portable than sdk.Int)
type Coin struct {
	Denom  string `json:"denom"`  // type, eg. "ATOM"
	Amount string `json:"amount"` // string encoing of decimal value, eg. "12.3456"
}

// CanoncialAddress uses standard base64 encoding, just use it as a label for developers
type CanonicalAddress = []byte

//------- Results / Msgs -------------

// CosmosResponse is the raw response from the init / handle calls
type CosmosResponse struct {
	Ok  Result `json:"ok"`
	Err string `json:"err"`
}

// Result defines the return value on a successful
type Result struct {
	// GasUsed is what is calculated from the VM, assuming it didn't run out of gas
	// This is set by the calling code, not the contract itself
	GasUsed uint64 `json:"gas_used"`
	// Messages comes directly from the contract and is it's request for action
	Messages []CosmosMsg `json:"messages"`
	// base64-encoded bytes to return as ABCI.Data field
	Data string `json:"data"`
	// log message to return over abci interface
	Log []LogAttribute `json:"log"`
}

// LogAttribute
type LogAttribute struct {
	Key   string `json:"key"`
	Value string `json:"value"`
}

// CosmosMsg is an rust enum and only (exactly) one of the fields should be set
// Should we do a cleaner approach in Go? (type/data?)
type CosmosMsg struct {
	Send     *SendMsg     `json:"send,omitempty"`
	Contract *ContractMsg `json:"contract,omitempty"`
	Opaque   *OpaqueMsg   `json:"opaque,omitempty"`
}

// SendMsg contains instructions for a Cosmos-SDK/SendMsg
// It has a fixed interface here and should be converted into the proper SDK format before dispatching
type SendMsg struct {
	FromAddress string `json:"from_address"`
	ToAddress   string `json:"to_address"`
	Amount      []Coin `json:"amount"`
}

// ContractMsg is used to call another defined contract on this chain.
// The calling contract requires the callee to be defined beforehand,
// and the address should have been defined in initialization.
// And we assume the developer tested the ABIs and coded them together.
//
// Since a contract is immutable once it is deployed, we don't need to transform this.
// If it was properly coded and worked once, it will continue to work throughout upgrades.
type ContractMsg struct {
	// ContractAddr is the sdk.AccAddress of the contract, which uniquely defines
	// the contract ID and instance ID. The sdk module should maintain a reverse lookup table.
	ContractAddr string `json:"contract_addr"`
	// Msg is assumed to be a json-encoded message, which will be passed directly
	// as `userMsg` when calling `Handle` on the above-defined contract
	Msg []byte `json:"msg"`
	// Send is an optional amount of coins this contract sends to the called contract
	Send []Coin `json:"send"`
}

// OpaqueMsg is some raw sdk-transaction that is passed in from a user and then relayed
// by the contract under some given conditions. These should never be created or
// inspected by the contract, but allows to build eg. multisig, governance in a contract
// and allow the end users to make use of all sdk functionality.
//
// An example is submitting a proposal for a vote. This is assumed to be correct (from the user)
// and if the contract determines the vote passed, the contract can then re-send it. If the chain
// updates, the client can submit a new proposal in the new format. Since this never comes from the
// contract itself, we don't need to worry about upgrading.
type OpaqueMsg struct {
	// Data is a custom msg that the sdk knows.
	// Generally the base64-encoded of go-amino binary encoding of an sdk.Msg implementation.
	// This should never be created by the contract, but allows for blindly passing through
	// temporary data.
	Data []byte `json:"data"`
}

//-------- Queries --------

type QueryResponse struct {
	Ok  []byte `json:"ok"`
	Err string `json:"err"`
}
