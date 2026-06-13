// yggaddr — валидатор вывода адреса Yggdrasil.
//
// Без аргументов: генерирует ключ и печатает seed/pubkey/addr.
// С аргументом <hex-pubkey>: печатает адрес Yggdrasil для данного публичного
// ключа ed25519 средствами самого yggdrasil-go.
//
// Используется для кросс-проверки нашей Rust-реализации addr_for_key.
package main

import (
	"crypto/ed25519"
	"crypto/rand"
	"encoding/hex"
	"fmt"
	"net"
	"os"

	"github.com/yggdrasil-network/yggdrasil-go/src/address"
)

func addrString(pub ed25519.PublicKey) string {
	a := address.AddrForKey(pub)
	return net.IP(a[:]).String()
}

func main() {
	if len(os.Args) >= 2 {
		pub, err := hex.DecodeString(os.Args[1])
		if err != nil || len(pub) != ed25519.PublicKeySize {
			fmt.Fprintln(os.Stderr, "expected 32-byte hex ed25519 public key")
			os.Exit(1)
		}
		fmt.Println(addrString(ed25519.PublicKey(pub)))
		return
	}

	pub, priv, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	fmt.Printf("seed   %s\n", hex.EncodeToString(priv.Seed()))
	fmt.Printf("pubkey %s\n", hex.EncodeToString(pub))
	fmt.Printf("addr   %s\n", addrString(pub))
}
