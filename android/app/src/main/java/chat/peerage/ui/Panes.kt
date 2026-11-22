package chat.peerage.ui

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.wrapContentWidth
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.key
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.res.stringResource
import chat.peerage.HoloSecondary
import chat.peerage.R
import chat.peerage.Tokens

/**
 * Две панели от 720 dp (ADR-005, поправка 2; макеты `home/SheetLand`, `home/SheetTablet`):
 * слева главная, справа стек экранов, между ними — фон. Каждая панель — тот же экран, что
 * на телефоне (шапка на фоне, лист под ней), поэтому экраны о раскладке не знают.
 */
@Composable
internal fun TwoPanes(list: @Composable () -> Unit, detail: @Composable () -> Unit) {
    BoxWithConstraints(Modifier.fillMaxSize()) {
        // Доля окна: на телефоне в ландшафте список уже, на планшете шире (макеты: 330 и 390).
        val listW = (maxWidth * 0.3f).coerceIn(Tokens.Size.listPaneMin, Tokens.Size.listPaneMax)
        // Поля по бокам — листы не упираются в край экрана (макеты: 8–12 по краям).
        Row(Modifier.fillMaxSize().padding(horizontal = Tokens.Space.s)) {
            Box(Modifier.width(listW).fillMaxHeight().clipToBounds()) { list() }
            Spacer(Modifier.width(Tokens.Space.s))
            // Сдвиг экранов при переходе не должен заезжать на список. Сверху — разница высот
            // шапок (главная: зона касания + отступы; экран: topBar), чтобы листы начинались
            // на одной линии, как в макетах.
            val align = Tokens.Size.touch + Tokens.Space.s * 2 - Tokens.Size.topBar
            Box(Modifier.weight(1f).fillMaxHeight().padding(top = align).clipToBounds()) { detail() }
        }
    }
}

/** Правая панель, пока ничего не выбрано: пустой лист со словами — пустота без них читается
 *  как «не загрузилось». Шапки нет: на этой высоте у соседнего списка своя. */
@Composable
internal fun NoChatPane() {
    Column(Modifier.fillMaxSize()) {
        Spacer(Modifier.height(Tokens.Size.topBar))
        Box(
            Modifier.weight(1f).fillMaxWidth()
                .clip(RoundedCornerShape(topStart = Tokens.Radius.panel, topEnd = Tokens.Radius.panel))
                .background(Tokens.Colors.panel),
            contentAlignment = Alignment.Center,
        ) {
            Text(stringResource(R.string.pane_no_chat), color = HoloSecondary, fontSize = Tokens.Font.body)
        }
    }
}

/** Новый [target] — содержимое проявляется заново (смена чата в правой панели). */
@Composable
internal fun FadeIn(target: Any?, content: @Composable () -> Unit) {
    key(target) {
        val alpha = remember { Animatable(0f) }
        LaunchedEffect(Unit) { alpha.animateTo(1f, tween(Tokens.Anim.normal)) }
        Box(Modifier.fillMaxSize().graphicsLayer { this.alpha = alpha.value }) { content() }
    }
}

/** Лента и поле ввода не шире [Tokens.Size.feedMax] и по центру своей панели. На телефоне
 *  ничего не меняет: панель уже предела. */
internal fun Modifier.feedWidth(): Modifier =
    fillMaxWidth().wrapContentWidth(Alignment.CenterHorizontally).widthIn(max = Tokens.Size.feedMax).fillMaxWidth()
