// libygg — C-разделяемая библиотека: тонкая обёртка вокруг ядра yggdrasil-go.
//
// Экспортирует датаграммный API поверх mesh-сети Yggdrasil, адресуемый по
// публичному ключу ed25519 узла. TUN не используется — встраивается как
// библиотека, не требует прав root. Линкуется из Rust (crate p2p-ygg).
//
// Сборка: go build -buildmode=c-shared -o libygg.so ./libygg
package main

/*
#include <stdint.h>
*/
import "C"

import (
	"crypto/ed25519"
	"io"
	"net/url"
	"os"
	"sync"
	"unsafe"

	iwt "github.com/Arceliar/ironwood/types"
	"github.com/gologme/log"
	"github.com/yggdrasil-network/yggdrasil-go/src/config"
	"github.com/yggdrasil-network/yggdrasil-go/src/core"
)

// newLogger: тихий логгер по умолчанию; при YGG_DEBUG=1 — в stderr со всеми уровнями.
func newLogger() *log.Logger {
	if os.Getenv("YGG_DEBUG") == "" {
		return log.New(io.Discard, "", 0)
	}
	logger := log.New(os.Stderr, "[ygg] ", log.Flags())
	logger.EnableLevel("error")
	logger.EnableLevel("warn")
	logger.EnableLevel("info")
	return logger
}

type node struct {
	core *core.Core
}

var (
	mu     sync.Mutex
	nodes  = map[uint64]*node{}
	nextID uint64 = 1
)

func register(n *node) uint64 {
	mu.Lock()
	defer mu.Unlock()
	id := nextID
	nextID++
	nodes[id] = n
	return id
}

func get(h C.uint64_t) *node {
	mu.Lock()
	defer mu.Unlock()
	return nodes[uint64(h)]
}

// copyOut копирует b в C-буфер out (вместимостью outCap). Возвращает длину
// или -1 если не помещается.
func copyOut(out unsafe.Pointer, outCap C.int, b []byte) C.int {
	if C.int(len(b)) > outCap {
		return -1
	}
	dst := unsafe.Slice((*byte)(out), int(outCap))
	copy(dst, b)
	return C.int(len(b))
}

//export ygg_start
// Запускает узел Yggdrasil из 32-байтного seed ed25519. Возвращает handle (>0)
// или 0 при ошибке.
func ygg_start(seedPtr *C.uint8_t, seedLen C.int) C.uint64_t {
	if seedLen != ed25519.SeedSize {
		return 0
	}
	seed := C.GoBytes(unsafe.Pointer(seedPtr), seedLen)
	priv := ed25519.NewKeyFromSeed(seed)

	cfg := config.GenerateConfig()
	cfg.PrivateKey = config.KeyBytes(priv)
	if err := cfg.GenerateSelfSignedCertificate(); err != nil {
		return 0
	}
	logger := newLogger()
	c, err := core.New(cfg.Certificate, logger)
	if err != nil {
		return 0
	}
	return C.uint64_t(register(&node{core: c}))
}

//export ygg_address
// Записывает строковый IPv6-адрес Yggdrasil узла в out. Возвращает длину или -1.
func ygg_address(h C.uint64_t, out *C.char, outCap C.int) C.int {
	n := get(h)
	if n == nil {
		return -1
	}
	return copyOut(unsafe.Pointer(out), outCap, []byte(n.core.Address().String()))
}

//export ygg_public_key
// Записывает 32-байтный публичный ключ ed25519 узла в out. Возвращает 32 или -1.
func ygg_public_key(h C.uint64_t, out *C.uint8_t) C.int {
	n := get(h)
	if n == nil {
		return -1
	}
	return copyOut(unsafe.Pointer(out), 32, []byte(n.core.PublicKey()))
}

//export ygg_listen
// Поднимает слушатель по URI (напр. "tls://127.0.0.1:0"). В out пишет
// фактический URI (с разрешённым портом). Возвращает длину или -1.
func ygg_listen(h C.uint64_t, uri *C.char, out *C.char, outCap C.int) C.int {
	n := get(h)
	if n == nil {
		return -1
	}
	u, err := url.Parse(C.GoString(uri))
	if err != nil {
		return -1
	}
	l, err := n.core.Listen(u, "")
	if err != nil {
		return -1
	}
	resolved := u.Scheme + "://" + l.Addr().String()
	return copyOut(unsafe.Pointer(out), outCap, []byte(resolved))
}

//export ygg_peer_count
// Число активных пиров (прямых соединений).
func ygg_peer_count(h C.uint64_t) C.int {
	n := get(h)
	if n == nil {
		return -1
	}
	return C.int(len(n.core.GetPeers()))
}

//export ygg_tree_count
// Число узлов в дереве маршрутизации (>1 означает, что сеть согласована).
func ygg_tree_count(h C.uint64_t) C.int {
	n := get(h)
	if n == nil {
		return -1
	}
	return C.int(len(n.core.GetTree()))
}

//export ygg_add_peer
// Добавляет пир по URI (напр. "tls://host:port"). Возвращает 0 или -1.
func ygg_add_peer(h C.uint64_t, uri *C.char) C.int {
	n := get(h)
	if n == nil {
		return -1
	}
	u, err := url.Parse(C.GoString(uri))
	if err != nil {
		return -1
	}
	if err := n.core.AddPeer(u, ""); err != nil {
		return -1
	}
	return 0
}

//export ygg_write
// Отправляет датаграмму data узлу с публичным ключом dstPub (32 байта).
// Возвращает число записанных байт или -1.
func ygg_write(h C.uint64_t, dstPub *C.uint8_t, data *C.uint8_t, dataLen C.int) C.int {
	n := get(h)
	if n == nil {
		return -1
	}
	pub := C.GoBytes(unsafe.Pointer(dstPub), 32)
	buf := C.GoBytes(unsafe.Pointer(data), dataLen)
	w, err := n.core.WriteTo(buf, iwt.Addr(ed25519.PublicKey(pub)))
	if err != nil {
		return -1
	}
	return C.int(w)
}

//export ygg_read
// Блокирующе читает одну датаграмму в out (вместимость outCap). В srcPub (32
// байта) пишет публичный ключ отправителя. Возвращает число байт или -1.
func ygg_read(h C.uint64_t, out *C.uint8_t, outCap C.int, srcPub *C.uint8_t) C.int {
	n := get(h)
	if n == nil {
		return -1
	}
	buf := make([]byte, 65535)
	r, from, err := n.core.ReadFrom(buf)
	if err != nil {
		return -1
	}
	if a, ok := from.(iwt.Addr); ok {
		copyOut(unsafe.Pointer(srcPub), 32, []byte(a))
	}
	return copyOut(unsafe.Pointer(out), outCap, buf[:r])
}

//export ygg_stop
// Останавливает узел и освобождает handle.
func ygg_stop(h C.uint64_t) {
	mu.Lock()
	n := nodes[uint64(h)]
	delete(nodes, uint64(h))
	mu.Unlock()
	if n != nil {
		n.core.Stop()
	}
}

func main() {}
