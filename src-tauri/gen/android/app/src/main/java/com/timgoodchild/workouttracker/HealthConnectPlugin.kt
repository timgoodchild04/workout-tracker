package com.timgoodchild.workouttracker

import android.app.Activity
import androidx.activity.result.ActivityResult
import androidx.health.connect.client.HealthConnectClient
import androidx.health.connect.client.PermissionController
import androidx.health.connect.client.permission.HealthPermission
import androidx.health.connect.client.records.ExerciseSessionRecord
import androidx.health.connect.client.records.HeartRateRecord
import androidx.health.connect.client.request.ReadRecordsRequest
import androidx.health.connect.client.time.TimeRangeFilter
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import java.time.Instant

@InvokeArg
class RowingArgs {
  /** The day to look in, as epoch milliseconds (local midnight to midnight). */
  var start: Long = 0
  var end: Long = 0
}

/**
 * Reads the watch's rowing workout and its heart rate from Health Connect (the Pixel
 * Watch's companion app writes both there). Read-only: nothing is written back.
 */
@TauriPlugin
class HealthConnectPlugin(private val activity: Activity) : Plugin(activity) {
  private val permissions = setOf(
    HealthPermission.getReadPermission(ExerciseSessionRecord::class),
    HealthPermission.getReadPermission(HeartRateRecord::class),
  )
  private val contract = PermissionController.createRequestPermissionResultContract()
  private val scope = CoroutineScope(Dispatchers.IO)

  /** The day's rowing workout: asks for permission first if it hasn't been given. */
  @Command
  fun readRowing(invoke: Invoke) {
    when (HealthConnectClient.getSdkStatus(activity)) {
      HealthConnectClient.SDK_AVAILABLE -> {}
      HealthConnectClient.SDK_UNAVAILABLE_PROVIDER_UPDATE_REQUIRED ->
        return invoke.reject("Health Connect needs updating. Update it from the Play Store and try again.")
      else -> return invoke.reject("Health Connect isn't available on this phone.")
    }
    val client = HealthConnectClient.getOrCreate(activity)
    scope.launch {
      try {
        if (client.permissionController.getGrantedPermissions().containsAll(permissions)) {
          read(client, invoke)
        } else {
          activity.runOnUiThread {
            startActivityForResult(invoke, contract.createIntent(activity, permissions), "permissionResult")
          }
        }
      } catch (e: Exception) {
        invoke.reject("Couldn't read Health Connect: ${e.message}")
      }
    }
  }

  @ActivityCallback
  private fun permissionResult(invoke: Invoke, result: ActivityResult) {
    val granted = contract.parseResult(result.resultCode, result.data)
    if (!granted.containsAll(permissions)) {
      return invoke.reject("Heart rate and workouts weren't allowed. Allow them in Settings → Health Connect → App permissions → Workout Tracker.")
    }
    scope.launch {
      try {
        read(HealthConnectClient.getOrCreate(activity), invoke)
      } catch (e: Exception) {
        invoke.reject("Couldn't read Health Connect: ${e.message}")
      }
    }
  }

  private suspend fun read(client: HealthConnectClient, invoke: Invoke) {
    val args = invoke.parseArgs(RowingArgs::class.java)
    val day = TimeRangeFilter.between(Instant.ofEpochMilli(args.start), Instant.ofEpochMilli(args.end))
    val rowing = setOf(ExerciseSessionRecord.EXERCISE_TYPE_ROWING_MACHINE, ExerciseSessionRecord.EXERCISE_TYPE_ROWING)
    val sessions = client.readRecords(ReadRecordsRequest(ExerciseSessionRecord::class, day)).records
      .filter { it.exerciseType in rowing }
    val out = JSObject()
    // The longest rowing workout that day (a stray short one is usually a false start).
    val session = sessions.maxByOrNull { it.endTime.toEpochMilli() - it.startTime.toEpochMilli() }
    if (session == null) {
      out.put("found", false)
      return invoke.resolve(out)
    }
    val bpm = mutableListOf<Long>()
    var page: String? = null
    do {
      val res = client.readRecords(
        ReadRecordsRequest(HeartRateRecord::class, TimeRangeFilter.between(session.startTime, session.endTime), pageToken = page)
      )
      res.records.flatMap { it.samples }
        .filter { !it.time.isBefore(session.startTime) && !it.time.isAfter(session.endTime) }
        .forEach { bpm.add(it.beatsPerMinute) }
      page = res.pageToken
    } while (page != null)
    out.put("found", true)
    out.put("start", session.startTime.toEpochMilli())
    out.put("end", session.endTime.toEpochMilli())
    out.put("sessions", sessions.size)
    out.put("samples", bpm.size)
    if (bpm.isNotEmpty()) {
      out.put("avg_hr", Math.round(bpm.average()))
      out.put("max_hr", bpm.max())
    }
    invoke.resolve(out)
  }
}
