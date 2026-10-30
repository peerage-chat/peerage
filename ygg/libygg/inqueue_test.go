package main

import "testing"

func pkt(n int) inPkt { return inPkt{data: make([]byte, n)} }

// Под наплывом крупных датаграмм очередь держит не больше своего лимита байт, а не
// 512 × 64 КБ (R-114); забранное освобождает место.
func TestInQueueBoundsBytes(t *testing.T) {
	q := newInQueue(512, 256*1024)
	n := 0
	for q.offer(pkt(64 * 1024)) {
		n++
	}
	if n != 4 {
		t.Fatalf("вошло %d по 64 КБ, ждали 4", n)
	}
	if _, ok := q.take(); !ok {
		t.Fatal("очередь пуста")
	}
	if !q.offer(pkt(64 * 1024)) {
		t.Fatal("забрали один — место для одного")
	}
}

// Мелких — не больше счётного предела, сколько бы байт ни оставалось.
func TestInQueueBoundsCount(t *testing.T) {
	q := newInQueue(3, 1<<20)
	for i := 0; i < 3; i++ {
		if !q.offer(pkt(10)) {
			t.Fatalf("пакет %d не вошёл", i)
		}
	}
	if q.offer(pkt(10)) {
		t.Fatal("четвёртый вошёл сверх предела")
	}
}

// Закрытая очередь отдаёт оставшееся, а потом — признак конца.
func TestInQueueCloseDrains(t *testing.T) {
	q := newInQueue(4, 1<<20)
	q.offer(pkt(1))
	q.close()
	if _, ok := q.take(); !ok {
		t.Fatal("оставшееся потерялось")
	}
	if _, ok := q.take(); ok {
		t.Fatal("после конца — пакет")
	}
}
