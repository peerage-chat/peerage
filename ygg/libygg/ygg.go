// libygg — C-разделяемая библиотека: тонкая обёртка вокруг ядра yggdrasil-go.
//
// Экспортирует датаграммный API поверх mesh-сети Yggdrasil, адресуемый по
// публичному ключу ed25519 узла. TUN не используется — встраивается как
// библиотека, не требует прав root. Линкуется из Rust (crate peerage-ygg).
//
// Сборка: go build -buildmode=c-shared -o libygg.so ./libygg
package main

/*
#include <stdint.h>
*/
import "C"

import (
	"crypto/ed25519"
	"encoding/hex"
	"encoding/json"
	"io"
	"net/url"
	"os"
	"sync"
	"sync/atomic"
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

// writeReq — запрос на отправку датаграммы.
type writeReq struct {
	dst  iwt.Addr
	data []byte
}

// inPkt — принятая датаграмма, ждущая ygg_read.
type inPkt struct {
	from      [32]byte
	fromKnown bool
	data      []byte
}

type node struct {
	core *core.Core
	// Надёжная очередь: чат, ключи, управление (ygg_write). При заторе блокирует
	// отправителя — терять их нельзя.
	reliableCh chan writeReq
	// Медиа звонка (ygg_write_media) — реалтайм и запечатано: класс пакета по
	// байтам не виден, его называет отправитель. Никогда не блокирует: при заторе
	// пакет выбрасывается (опоздавшее медиа бесполезно; LiteRTC повторит важное и
	// сбавит темп). Блокировка здесь останавливала бы потоки звонка целиком — и
	// звук, и приём. Срочная очередь — звук и служебные пакеты звонка, объёмная —
	// видео.
	urgentCh chan writeReq
	bulkCh   chan writeReq
	stopCh   chan struct{}

	// Чёрный список источников: пакеты от этих ключей отбрасываются ЗДЕСЬ, не
	// пересекая FFI. Приёмный бюджет в Rust режет их и так, но каждый пакет до
	// него стоит копирования через границу и разбора заголовка — под флудом это
	// заметно. Байты с провода это не убирает (см. threat-model N8), убирает
	// только работу над ними.
	blockedMu sync.RWMutex
	blocked   map[[32]byte]struct{}
	dropped   atomic.Uint64
	// Принятое, но ещё не забранное ygg_read (см. readLoop). Закрыт — узел
	// остановлен.
	inCh chan inPkt
}

// isBlocked — источник в чёрном списке? Дешёвая проверка под RLock; при пустом
// списке (обычный случай) стоит одну атомарную операцию.
func (n *node) isBlocked(key []byte) bool {
	if len(key) != 32 {
		return false
	}
	n.blockedMu.RLock()
	defer n.blockedMu.RUnlock()
	if len(n.blocked) == 0 {
		return false
	}
	var k [32]byte
	copy(k[:], key)
	_, ok := n.blocked[k]
	return ok
}

// writerLoop — единственная долгоживущая горутина записи на узел.
// Приоритет: срочное медиа и надёжные пакеты — дочиста, видео — по одному между ними.
func writerLoop(n *node) {
	for {
		for more := true; more; {
			select {
			case req := <-n.urgentCh:
				n.core.WriteTo(req.data, req.dst) //nolint:errcheck
			case req := <-n.reliableCh:
				n.core.WriteTo(req.data, req.dst) //nolint:errcheck
			default:
				more = false
			}
		}
		select {
		case <-n.stopCh:
			return
		case req := <-n.urgentCh:
			n.core.WriteTo(req.data, req.dst) //nolint:errcheck
		case req := <-n.reliableCh:
			n.core.WriteTo(req.data, req.dst) //nolint:errcheck
		case req := <-n.bulkCh:
			n.core.WriteTo(req.data, req.dst) //nolint:errcheck
		}
	}
}

// readLoop — единственная горутина чтения из ядра: забирает входящие без остановки
// и кладёт в inCh, откуда их берёт ygg_read. Читать надо всегда: ironwood отдаёт
// датаграмму через актор сессии с её отправителем и, пока её не забрали, держит
// этот актор — а через него же идёт и ЗАПИСЬ этому собеседнику. Когда ygg_read
// читал из ядра сам, медленный потребитель (поток приёма в Rust, который и сам
// пишет надёжно, ожидая места в очереди записи) замыкал круг: запись ждала
// чтения, чтение — записи, и под затором звонок вставал на минуты. Очередь
// полна — пакет выброшен: чат повторится, медиа опоздало бы всё равно.
func readLoop(n *node) {
	defer close(n.inCh)
	// Один буфер на узел: выделять 64 КБ на каждую датаграмму под флудом —
	// давление на GC на ровном месте.
	buf := make([]byte, 65535)
	for {
		r, from, err := n.core.ReadFrom(buf)
		if err != nil {
			return
		}
		p := inPkt{data: append([]byte(nil), buf[:r]...)}
		if a, ok := from.(iwt.Addr); ok {
			if n.isBlocked([]byte(a)) {
				n.dropped.Add(1)
				continue // молча дальше: ни копирования через границу, ни разбора
			}
			copy(p.from[:], a)
			p.fromKnown = true
		}
		select {
		case n.inCh <- p:
		default:
		}
	}
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
	n := &node{
		core:       c,
		reliableCh: make(chan writeReq, 128),
		urgentCh:   make(chan writeReq, 128),
		bulkCh:     make(chan writeReq, 64),
		stopCh:     make(chan struct{}),
		blocked:    make(map[[32]byte]struct{}),
		// ~2.5 с видео на полной полосе.
		inCh: make(chan inPkt, 512),
	}
	go writerLoop(n)
	go readLoop(n)
	return C.uint64_t(register(n))
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

//export ygg_peers_json
// JSON-массив активных пиров для UI/стата сети:
// [{key, inbound, up, latency_ms, rx_rate, tx_rate, rx_bytes, tx_bytes, uptime_s}].
// -1 если не влезло.
func ygg_peers_json(h C.uint64_t, out *C.char, outCap C.int) C.int {
	n := get(h)
	if n == nil {
		return -1
	}
	type peerJSON struct {
		Key       string  `json:"key"`
		Inbound   bool    `json:"inbound"`
		Up        bool    `json:"up"`
		LatencyMs float64 `json:"latency_ms"`
		RxRate    uint64  `json:"rx_rate"`
		TxRate    uint64  `json:"tx_rate"`
		// Накопительные счётчики: скорости сглажены и на границах измерения
		// врут, а разница байт между двумя снимками — точная величина.
		RxBytes uint64  `json:"rx_bytes"`
		TxBytes uint64  `json:"tx_bytes"`
		UptimeS float64 `json:"uptime_s"`
	}
	peers := n.core.GetPeers()
	arr := make([]peerJSON, 0, len(peers))
	for _, p := range peers {
		arr = append(arr, peerJSON{
			Key:       hex.EncodeToString(p.Key),
			Inbound:   p.Inbound,
			Up:        p.Up,
			LatencyMs: float64(p.Latency.Microseconds()) / 1000.0,
			RxRate:    p.RXRate,
			TxRate:    p.TXRate,
			RxBytes:   p.RXBytes,
			TxBytes:   p.TXBytes,
			UptimeS:   p.Uptime.Seconds(),
		})
	}
	b, err := json.Marshal(arr)
	if err != nil {
		return -1
	}
	return copyOut(unsafe.Pointer(out), outCap, b)
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
// Возвращает dataLen при успехе или -1 если узел не найден.
//
// Надёжная запись (чат, ключи, управление): при заторе ждёт места в очереди — терять
// эти пакеты нельзя. Медиа звонка идёт через ygg_write_media.
func ygg_write(h C.uint64_t, dstPub *C.uint8_t, data *C.uint8_t, dataLen C.int) C.int {
	n := get(h)
	if n == nil {
		return -1
	}
	pub := C.GoBytes(unsafe.Pointer(dstPub), 32)
	buf := C.GoBytes(unsafe.Pointer(data), dataLen)
	dst := iwt.Addr(ed25519.PublicKey(pub))
	req := writeReq{dst: dst, data: buf}

	select {
	case n.reliableCh <- req:
	case <-n.stopCh:
		return -1
	}
	return dataLen
}

//export ygg_write_media
// Отправляет медиа-датаграмму звонка. Никогда не блокирует: очередь полна — пакет
// выброшен, возвращается 0. urgent != 0 — звук и служебные пакеты (своя очередь,
// пишется раньше видео). Возвращает dataLen, 0 (выброшен) или -1 (узел не найден).
func ygg_write_media(h C.uint64_t, dstPub *C.uint8_t, data *C.uint8_t, dataLen C.int, urgent C.int) C.int {
	n := get(h)
	if n == nil {
		return -1
	}
	pub := C.GoBytes(unsafe.Pointer(dstPub), 32)
	buf := C.GoBytes(unsafe.Pointer(data), dataLen)
	req := writeReq{dst: iwt.Addr(ed25519.PublicKey(pub)), data: buf}
	ch := n.bulkCh
	if urgent != 0 {
		ch = n.urgentCh
	}
	select {
	case ch <- req:
		return dataLen
	default:
		return 0
	}
}

//export ygg_read
// Блокирующе читает одну датаграмму в out (вместимость outCap). В srcPub (32
// байта) пишет публичный ключ отправителя. Возвращает число байт или -1.
//
// Пакеты от заблокированных источников (ygg_block) отбрасывает readLoop, и они
// НЕ пересекают FFI: вызывающему они не видны.
func ygg_read(h C.uint64_t, out *C.uint8_t, outCap C.int, srcPub *C.uint8_t) C.int {
	n := get(h)
	if n == nil {
		return -1
	}
	p, ok := <-n.inCh
	if !ok {
		return -1
	}
	if p.fromKnown {
		copyOut(unsafe.Pointer(srcPub), 32, p.from[:])
	}
	return copyOut(unsafe.Pointer(out), outCap, p.data)
}

//export ygg_block
// Добавить публичный ключ в чёрный список: его пакеты будут отбрасываться до
// пересечения FFI. Идемпотентно. Возвращает 0 при успехе, -1 если узла нет.
func ygg_block(h C.uint64_t, keyPtr *C.uint8_t) C.int {
	n := get(h)
	if n == nil {
		return -1
	}
	key := C.GoBytes(unsafe.Pointer(keyPtr), 32)
	var k [32]byte
	copy(k[:], key)
	n.blockedMu.Lock()
	n.blocked[k] = struct{}{}
	n.blockedMu.Unlock()
	return 0
}

//export ygg_unblock
// Убрать ключ из чёрного списка. Идемпотентно.
func ygg_unblock(h C.uint64_t, keyPtr *C.uint8_t) C.int {
	n := get(h)
	if n == nil {
		return -1
	}
	key := C.GoBytes(unsafe.Pointer(keyPtr), 32)
	var k [32]byte
	copy(k[:], key)
	n.blockedMu.Lock()
	delete(n.blocked, k)
	n.blockedMu.Unlock()
	return 0
}

//export ygg_dropped
// Сколько пакетов отброшено чёрным списком за время жизни узла (для диагностики
// и экрана «вас флудят»). -1 если узла нет.
func ygg_dropped(h C.uint64_t) C.int64_t {
	n := get(h)
	if n == nil {
		return -1
	}
	return C.int64_t(n.dropped.Load())
}

//export ygg_stop
// Останавливает узел и освобождает handle.
func ygg_stop(h C.uint64_t) {
	mu.Lock()
	n := nodes[uint64(h)]
	delete(nodes, uint64(h))
	mu.Unlock()
	if n != nil {
		close(n.stopCh) // останавливает writerLoop
		n.core.Stop()
	}
}

func main() {}
