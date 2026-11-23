package chat.peerage

import android.app.Application
import android.content.Context

/** Процесс приложения: только язык интерфейса ниже Android 13 ([AppLanguage.wrap]). */
class PeerageApp : Application() {
    override fun attachBaseContext(base: Context) = super.attachBaseContext(AppLanguage.wrap(base))
}
