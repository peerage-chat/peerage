package dev.p2p.messenger

import kotlinx.serialization.Serializable
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.json.Json

/**
 * Перевод ссылок на сообщения в формы, которые понимает движок.
 *
 * Внутри клиента ссылка одна — пара «id + автор» ([MsgRef], ADR-006). На проводе
 * форм две и они разные: личный канал спрашивает «моё ли это», групповой — «чей».
 * Перевод живёт здесь, отдельно от модели: это граница с движком, и её форма
 * зафиксирована тестами с обеих сторон (`androidjni` разбирает ровно эти поля).
 */
@Serializable
internal data class WireDmRef(val id: Long, val mine: Boolean)

@Serializable
internal data class WireGroupRef(val id: Long, val sender: String)

private val wire = Json { encodeDefaults = true }

/** Ссылки для личного канала: автор сворачивается в «моё/чужое» по своему ключу. */
internal fun dmRefsJson(refs: Collection<MsgRef>, meKey: String): String =
    wire.encodeToString(
        ListSerializer(WireDmRef.serializer()),
        refs.map { WireDmRef(it.id, it.sender == meKey) },
    )

/** Ссылки для группового канала: автор нужен явно — участников много. */
internal fun groupRefsJson(refs: Collection<MsgRef>): String =
    wire.encodeToString(
        ListSerializer(WireGroupRef.serializer()),
        refs.map { WireGroupRef(it.id, it.sender) },
    )
