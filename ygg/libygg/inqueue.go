package main

import "sync/atomic"

// inQueue — принятое, но ещё не забранное ygg_read (см. readLoop). Ограничена и по
// числу пакетов, и по байтам (R-114): readLoop вычерпывает ядро, не дожидаясь
// потребителя, и под наплывом крупных датаграмм очередь только по числу держала бы
// до 512 × 64 КБ. Не влезло — пакет выброшен: чат повторится, медиа опоздало бы всё
// равно.
type inQueue struct {
	ch       chan inPkt
	bytes    atomic.Int64
	maxBytes int64
}

func newInQueue(maxPackets int, maxBytes int64) *inQueue {
	return &inQueue{ch: make(chan inPkt, maxPackets), maxBytes: maxBytes}
}

// offer кладёт пакет, если влезает; false — выброшен. Писатель один (readLoop).
func (q *inQueue) offer(p inPkt) bool {
	size := int64(len(p.data))
	if q.bytes.Load()+size > q.maxBytes {
		return false
	}
	q.bytes.Add(size)
	select {
	case q.ch <- p:
		return true
	default:
		q.bytes.Add(-size)
		return false
	}
}

// take ждёт пакет; false — очередь закрыта и пуста.
func (q *inQueue) take() (inPkt, bool) {
	p, ok := <-q.ch
	if ok {
		q.bytes.Add(-int64(len(p.data)))
	}
	return p, ok
}

// close — писателя больше не будет; оставшееся ещё можно забрать.
func (q *inQueue) close() { close(q.ch) }
