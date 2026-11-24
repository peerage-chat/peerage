package chat.peerage

import android.app.Application
import android.content.Context

/** Процесс приложения: язык интерфейса ниже Android 13 ([AppLanguage.wrap]) и проверка обновлений. */
class PeerageApp : Application() {
    override fun attachBaseContext(base: Context) = super.attachBaseContext(AppLanguage.wrap(base))

    override fun onCreate() {
        super.onCreate()
        Updates.init(this)
    }
}
