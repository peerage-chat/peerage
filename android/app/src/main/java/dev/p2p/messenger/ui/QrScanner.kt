@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package dev.p2p.messenger.ui

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.BackHandler
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.camera.core.CameraSelector
import androidx.camera.core.ExperimentalGetImage
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.ImageProxy
import androidx.camera.core.Preview
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.Chat
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.AttachFile
import androidx.compose.material.icons.filled.BatteryStd
import androidx.compose.material.icons.filled.Bolt
import androidx.compose.material.icons.filled.Call
import androidx.compose.material.icons.filled.CallEnd
import androidx.compose.material.icons.filled.Campaign
import androidx.compose.material.icons.filled.ChevronRight
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material.icons.filled.EmojiEmotions
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.Mic
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Create
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Done
import androidx.compose.material.icons.filled.DoneAll
import androidx.compose.material.icons.filled.Forum
import androidx.compose.material.icons.filled.Schedule
import androidx.compose.material.icons.filled.Group
import androidx.compose.material.icons.filled.GroupAdd
import androidx.compose.material.icons.filled.Info
import androidx.compose.material.icons.filled.Menu
import androidx.compose.material.icons.filled.MicOff
import androidx.compose.material.icons.filled.PersonAdd
import androidx.compose.material.icons.filled.QrCode2
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.filled.Verified
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.Share
import androidx.compose.material.icons.filled.Videocam
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalLifecycleOwner
import androidx.compose.ui.viewinterop.AndroidView
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.foundation.horizontalScroll
import androidx.compose.material.icons.filled.Headset
import androidx.compose.material3.ExperimentalMaterial3Api
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlin.math.abs
import dev.p2p.messenger.*

@Composable
internal fun QrScannerScreen(onResult: (String) -> Unit) {
    val ctx = LocalContext.current
    val lifecycleOwner = LocalLifecycleOwner.current

    val previewView = remember { PreviewView(ctx) }
    var cameraProvider: ProcessCameraProvider? = remember { null }

    LaunchedEffect(lifecycleOwner) {
        cameraProvider = withContext(Dispatchers.IO) {
            ProcessCameraProvider.getInstance(ctx).get()
        }
        val preview = Preview.Builder().build().also {
            it.setSurfaceProvider(previewView.surfaceProvider)
        }
        val analysis = ImageAnalysis.Builder()
            .setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST)
            .build()
        analysis.setAnalyzer(
            androidx.core.content.ContextCompat.getMainExecutor(ctx),
            QrAnalyzer(onResult),
        )
        cameraProvider?.unbindAll()
        cameraProvider?.bindToLifecycle(
            lifecycleOwner, CameraSelector.DEFAULT_BACK_CAMERA, preview, analysis,
        )
    }

    DisposableEffect(Unit) {
        onDispose { cameraProvider?.unbindAll() }
    }

    Box(Modifier.fillMaxSize()) {
        // Превью камеры
        AndroidView(factory = { previewView }, modifier = Modifier.fillMaxSize())

        // Тёмная маска: 4 прямоугольника вокруг прозрачного квадрата-прицела
        val boxSizePx = with(androidx.compose.ui.platform.LocalDensity.current) { 260.dp.toPx() }
        Canvas(Modifier.fillMaxSize()) {
            val l = (size.width - boxSizePx) / 2
            val t = (size.height - boxSizePx) / 2
            val r = l + boxSizePx
            val b = t + boxSizePx
            val dim = Color(0x99000000)
            drawRect(dim, topLeft = Offset.Zero, size = Size(size.width, t))          // верх
            drawRect(dim, topLeft = Offset(0f, t), size = Size(l, boxSizePx))         // лево
            drawRect(dim, topLeft = Offset(r, t), size = Size(size.width - r, boxSizePx)) // право
            drawRect(dim, topLeft = Offset(0f, b), size = Size(size.width, size.height - b)) // низ
        }

        // Рамка прицела
        Box(
            Modifier.size(260.dp).align(Alignment.Center)
                .border(2.dp, HoloAccent, RoundedCornerShape(12.dp)),
        )

        // Подсказка
        Text(
            "Наведите камеру на QR-код",
            color = Color.White,
            fontSize = 14.sp,
            modifier = Modifier.align(Alignment.BottomCenter).padding(bottom = 56.dp),
        )
    }
}

/** Анализатор кадров: ищет QR-код и вызывает [onResult] один раз при успехе. */
internal class QrAnalyzer(private val onResult: (String) -> Unit) : ImageAnalysis.Analyzer {
    private val reader = com.google.zxing.qrcode.QRCodeReader()
    @Volatile private var done = false

    @ExperimentalGetImage
    override fun analyze(proxy: ImageProxy) {
        if (done) { proxy.close(); return }
        val img = proxy.image
        if (img != null) {
            try {
                val plane = img.planes[0]
                val buf = plane.buffer
                val data = ByteArray(buf.remaining()).also { buf.get(it) }
                val src = com.google.zxing.PlanarYUVLuminanceSource(
                    data, proxy.width, proxy.height, 0, 0, proxy.width, proxy.height, false,
                )
                val result = reader.decode(com.google.zxing.BinaryBitmap(
                    com.google.zxing.common.HybridBinarizer(src),
                ))
                done = true
                onResult(result.text)
            } catch (_: com.google.zxing.NotFoundException) {
                // нет QR в этом кадре — норма
            }
        }
        proxy.close()
    }
}

/** Папки = умные фильтры (Discord-рейл иконок). key→(иконка, подпись). */

