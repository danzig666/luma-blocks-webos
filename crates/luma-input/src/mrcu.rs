//! Parsing of `luna://com.webos.service.mrcu` replies into [`MotionSample`]s and classified
//! errors. Pure functions, so the TV adapter and the host tests share them.
//!
//! Field names and units follow LG's motion-sensor documentation (webOS TV 24+ `sensor2`
//! family, and the older `sensor/getSensorData`). They are re-checked against real samples
//! in the on-TV diagnostics screen; anything unexpected is reported as malformed rather than
//! guessed.

use crate::math::{Quat, Vec3};
use crate::sample::MotionSample;
use serde_json::Value;

pub const SERVICE: &str = "luna://com.webos.service.mrcu";
/// Fields requested from `sensor2/getSensorEventData` (multiple types may be combined).
pub const SENSOR2_TYPES: &str = "gameRotationVector,gyroscope,acceleration,linearAcceleration,coordinate";
/// Legacy `callbackInterval` encoding: 1 = 100/s, 2 = 50/s, 3 = 33/s, 4 = 10/s.
pub fn legacy_interval_code(target_hz: f32) -> u32 {
    if target_hz >= 75.0 {
        1
    } else if target_hz >= 40.0 {
        2
    } else if target_hz >= 20.0 {
        3
    } else {
        4
    }
}

/// Classified failure of an mrcu call. Raw text is kept for the developer screen only.
#[derive(Debug, Clone, PartialEq)]
pub enum MrcuError {
    /// The method or service does not exist on this firmware (errorCode -1, unknown method).
    Unsupported(String),
    /// The bus refused the call for this application's identity.
    PermissionDenied(String),
    /// The remote's sensor is asleep / not ready (1003 in sensor2, 1001 in legacy).
    Sleeping,
    /// 1004: this client already subscribed.
    AlreadySubscribed,
    /// 1005: too many clients.
    TooManyClients,
    /// 1006: interval outside the supported range.
    BadInterval,
    InvalidParameter(String),
    SubscribeFailed(String),
    /// Reply did not have the documented shape.
    Malformed(String),
    Other {
        code: i64,
        text: String,
    },
}

impl MrcuError {
    /// Short, human label for ordinary UI (no raw platform text).
    pub fn user_label(&self) -> &'static str {
        match self {
            MrcuError::Unsupported(_) => "Motion sensing is not available on this TV",
            MrcuError::PermissionDenied(_) => "Motion sensing is not permitted for this app",
            MrcuError::Sleeping => "The remote is asleep. Move it to wake it up",
            MrcuError::AlreadySubscribed => "Motion stream already active",
            MrcuError::TooManyClients => "Too many apps are using the remote's sensor",
            MrcuError::BadInterval => "Requested sample rate was refused",
            MrcuError::InvalidParameter(_) => "Motion request was refused",
            MrcuError::SubscribeFailed(_) => "Could not start the motion stream",
            MrcuError::Malformed(_) => "Unexpected motion data",
            MrcuError::Other { .. } => "Motion sensing failed",
        }
    }
    pub fn detail(&self) -> String {
        format!("{self:?}")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Reply {
    /// Subscription accepted (`subscribed: true`) with no data yet.
    Subscribed,
    /// A data callback.
    Data(MotionSample),
    /// Plain success (`returnValue: true`) without data, e.g. setSensorInterval.
    Ok,
    Error(MrcuError),
}

fn num(v: &Value) -> Option<f32> {
    v.as_f64().map(|f| f as f32).filter(|f| f.is_finite())
}

fn vec3(v: Option<&Value>) -> Option<Vec3> {
    let v = v?;
    Some(Vec3::new(num(v.get("x")?)?, num(v.get("y")?)?, num(v.get("z")?)?))
}

fn error_code(v: &Value) -> Option<i64> {
    let c = v.get("errorCode")?;
    // Documented as a string, shown as a number in examples: accept both.
    c.as_i64()
        .or_else(|| c.as_str().and_then(|s| s.trim().parse().ok()))
}

/// Classify a `returnValue: false` reply. `legacy` selects the legacy code table.
pub fn classify_error(v: &Value, legacy: bool) -> MrcuError {
    let code = error_code(v).unwrap_or(0);
    let text = v
        .get("errorText")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_owned();
    let lower = text.to_ascii_lowercase();
    if lower.contains("denied") || lower.contains("permission") || lower.contains("not allowed") {
        return MrcuError::PermissionDenied(text);
    }
    if lower.contains("unknown method")
        || lower.contains("service does not exist")
        || lower.contains("not supported")
        || lower.contains("no such")
    {
        return MrcuError::Unsupported(text);
    }
    if lower.contains("not ready") || lower.contains("sleep") {
        return MrcuError::Sleeping;
    }
    if legacy {
        return match code {
            1001 => MrcuError::Sleeping,
            1002 => MrcuError::BadInterval,
            1003 => MrcuError::SubscribeFailed(text),
            1004 => MrcuError::InvalidParameter(text),
            -1 => MrcuError::Unsupported(text),
            _ => MrcuError::Other { code, text },
        };
    }
    match code {
        1001 => MrcuError::SubscribeFailed(text),
        1002 => MrcuError::InvalidParameter(text),
        1003 => MrcuError::Sleeping,
        1004 => MrcuError::AlreadySubscribed,
        1005 => MrcuError::TooManyClients,
        1006 => MrcuError::BadInterval,
        // LG: "If returnValue is false and errorCode is -1, the platform version ... probably
        // does not support the Motion Sensor API."
        -1 => MrcuError::Unsupported(text),
        _ => MrcuError::Other { code, text },
    }
}

fn parse_common(payload: &str, legacy: bool) -> Result<Value, Reply> {
    let v: Value = serde_json::from_str(payload)
        .map_err(|e| Reply::Error(MrcuError::Malformed(format!("json: {e}"))))?;
    if !v.is_object() {
        return Err(Reply::Error(MrcuError::Malformed("not an object".into())));
    }
    if v.get("returnValue").and_then(|r| r.as_bool()) == Some(false) {
        return Err(Reply::Error(classify_error(&v, legacy)));
    }
    if v.get("subscribed").and_then(|s| s.as_bool()) == Some(false) {
        return Err(Reply::Error(MrcuError::SubscribeFailed(
            "subscribed: false".into(),
        )));
    }
    Ok(v)
}

/// Parse a `sensor2/getSensorEventData` reply.
pub fn parse_sensor2(payload: &str, t_us: u64) -> Reply {
    let v = match parse_common(payload, false) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let quat = v.get("gameRotationVector").and_then(|g| {
        Some(Quat::new(
            num(g.get("w")?)?,
            num(g.get("x")?)?,
            num(g.get("y")?)?,
            num(g.get("z")?)?,
        ))
    });
    let s = MotionSample {
        t_us,
        quat,
        gyro: vec3(v.get("gyroscope")),
        // The request type is "acceleration" but the documented reply key is "accelerometer".
        accel: vec3(v.get("accelerometer")).or_else(|| vec3(v.get("acceleration"))),
        linear_accel: vec3(v.get("linearAcceleration")),
        cursor: v
            .get("coordinate")
            .and_then(|c| Some((num(c.get("x")?)?, num(c.get("y")?)?))),
        interval_ms: v.get("interval").and_then(num),
    };
    finish(s, &v)
}

/// Legacy quaternion component order. LG's table maps q0..q3 to x, y, z, w; this stays
/// switchable from the developer screen until verified on hardware.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub enum LegacyQuatOrder {
    #[default]
    Xyzw,
    Wxyz,
}

/// Parse a legacy `sensor/getSensorData` reply.
pub fn parse_legacy(payload: &str, t_us: u64, order: LegacyQuatOrder) -> Reply {
    let v = match parse_common(payload, true) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let quat = v.get("quaternion").and_then(|q| {
        let a = [
            num(q.get("q0")?)?,
            num(q.get("q1")?)?,
            num(q.get("q2")?)?,
            num(q.get("q3")?)?,
        ];
        Some(match order {
            LegacyQuatOrder::Xyzw => Quat::new(a[3], a[0], a[1], a[2]),
            LegacyQuatOrder::Wxyz => Quat::new(a[0], a[1], a[2], a[3]),
        })
    });
    let s = MotionSample {
        t_us,
        quat,
        gyro: vec3(v.get("gyroscope")),
        accel: vec3(v.get("acceleration")),
        linear_accel: None,
        cursor: v
            .get("coordinate")
            .and_then(|c| Some((num(c.get("x")?)?, num(c.get("y")?)?))),
        interval_ms: None,
    };
    finish(s, &v)
}

fn finish(s: MotionSample, v: &Value) -> Reply {
    let clean = s.sanitized();
    if clean.quat.is_none()
        && clean.gyro.is_none()
        && clean.accel.is_none()
        && clean.cursor.is_none()
        && clean.linear_accel.is_none()
    {
        if v.get("subscribed").and_then(|s| s.as_bool()) == Some(true) {
            return Reply::Subscribed;
        }
        if s.quat.is_some() || s.gyro.is_some() {
            return Reply::Error(MrcuError::Malformed("implausible values".into()));
        }
        return Reply::Ok;
    }
    Reply::Data(clean)
}

/// `sensor2/getSensorInterval` → (current, min, max) in ms.
pub fn parse_interval(payload: &str) -> Result<(u32, u32, u32), MrcuError> {
    let v = parse_common(payload, false).map_err(|r| match r {
        Reply::Error(e) => e,
        _ => MrcuError::Malformed("interval".into()),
    })?;
    let g = |k: &str| v.get(k).and_then(|x| x.as_f64()).map(|x| x as u32);
    match (g("interval"), g("minInterval"), g("maxInterval")) {
        (Some(c), Some(mi), Some(ma)) if mi > 0 && mi <= ma => Ok((c, mi, ma)),
        _ => Err(MrcuError::Malformed("interval fields".into())),
    }
}

/// `sensor2/getSensorState` → isAlive.
pub fn parse_state(payload: &str) -> Result<bool, MrcuError> {
    let v = parse_common(payload, false).map_err(|r| match r {
        Reply::Error(e) => e,
        _ => MrcuError::Malformed("state".into()),
    })?;
    v.get("isAlive")
        .and_then(|a| a.as_bool())
        .ok_or_else(|| MrcuError::Malformed("isAlive".into()))
}

/// Choose an interval (ms) within the reported bounds for a target rate.
pub fn choose_interval(min_ms: u32, max_ms: u32, target_hz: f32) -> u32 {
    let want = (1000.0 / target_hz.max(1.0)).round() as u32;
    want.clamp(min_ms.max(1), max_ms.max(min_ms.max(1)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC_SENSOR2: &str = r#"{"returnValue":true,"gyroscopeUncalibrated":{"y_bias":0.0004,"z_bias":0.0037,"x_uncalib":-0.68,"y_uncalib":-0.14,"x_bias":0.0029,"z_uncalib":-0.12},"coordinate":{"y":1074,"x":99},"motionDetector":{"significantMotion":0},"interval":995.0,"gravity":{"z":9.70,"y":-0.85,"x":-0.16},"linearAcceleration":{"z":1.60,"y":0.98,"x":-0.23},"gyroscope":{"z":-0.129,"y":-0.145,"x":-0.684},"pedometer":{"stepDetected":0,"stepCount":4,"stepDetectedLatency":0},"gameRotationVector":{"z":0.61527740955352783,"w":0.78128117322921753,"y":-0.02904123067855835,"euler":{"pitch":7.0,"roll":-9.8,"yaw":-77.0},"x":0.10094617307186127},"accelerometer":{"z":11.31,"y":0.128,"x":-0.406}}"#;

    #[test]
    fn documented_sensor2_example_parses() {
        let Reply::Data(s) = parse_sensor2(DOC_SENSOR2, 5) else {
            panic!("expected data")
        };
        let q = s.quat.unwrap();
        assert!((q.w - 0.7813).abs() < 1e-3 && (q.z - 0.6153).abs() < 1e-3);
        assert!((s.gyro.unwrap().x + 0.684).abs() < 1e-3);
        assert!((s.accel.unwrap().z - 11.31).abs() < 1e-3);
        assert_eq!(s.cursor, Some((99.0, 1074.0)));
        assert_eq!(s.interval_ms, Some(995.0));
    }

    #[test]
    fn documented_legacy_example_parses() {
        let p = r#"{"deviceId":0,"coordinate":{"x":1101,"y":518},"gyroscope":{"z":-1.682723,"x":0.3041,"y":-0.582472},"acceleration":{"z":12.035406,"x":9.713932,"y":5.446675},"returnValue":true,"quaternion":{"q0":0.042376,"q1":0.088355,"q2":0.937461,"q3":0.334014}}"#;
        let Reply::Data(s) = parse_legacy(p, 1, LegacyQuatOrder::Xyzw) else {
            panic!()
        };
        let q = s.quat.unwrap();
        assert!((q.w - 0.334).abs() < 1e-3);
        assert!(s.linear_accel.is_none());
    }

    #[test]
    fn missing_fields_stay_missing() {
        let Reply::Data(s) = parse_sensor2(r#"{"returnValue":true,"gyroscope":{"x":0.1,"y":0,"z":0}}"#, 0)
        else {
            panic!()
        };
        assert!(s.quat.is_none() && s.accel.is_none() && s.cursor.is_none());
        // A partial vector is not silently completed with zeros.
        let r = parse_sensor2(r#"{"returnValue":true,"gyroscope":{"x":0.1}}"#, 0);
        assert_eq!(r, Reply::Ok);
    }

    #[test]
    fn error_classification() {
        let e = |p: &str| match parse_sensor2(p, 0) {
            Reply::Error(e) => e,
            r => panic!("{r:?}"),
        };
        assert_eq!(
            e(r#"{"returnValue":false,"errorCode":1003,"errorText":"Magic remote is not ready"}"#),
            MrcuError::Sleeping
        );
        assert_eq!(
            e(r#"{"returnValue":false,"errorCode":"1004","errorText":"Already add subscription"}"#),
            MrcuError::AlreadySubscribed
        );
        assert_eq!(
            e(r#"{"returnValue":false,"errorCode":1006,"errorText":"Wrong Callback Interval"}"#),
            MrcuError::BadInterval
        );
        assert!(matches!(
            e(
                r#"{"returnValue":false,"errorCode":-1,"errorText":"Unknown method \"getSensorEventData\" for category \"/sensor2\""}"#
            ),
            MrcuError::Unsupported(_)
        ));
        assert!(matches!(
            e(
                r#"{"returnValue":false,"errorCode":-1,"errorText":"Denied method call \"getSensorEventData\" for category \"/sensor2\""}"#
            ),
            MrcuError::PermissionDenied(_)
        ));
        assert!(matches!(e("garbage"), MrcuError::Malformed(_)));
        assert!(matches!(
            e(r#"{"returnValue":true,"subscribed":false}"#),
            MrcuError::SubscribeFailed(_)
        ));
    }

    #[test]
    fn subscribed_ack_and_implausible_data() {
        assert_eq!(
            parse_sensor2(r#"{"returnValue":true,"subscribed":true}"#, 0),
            Reply::Subscribed
        );
        let r = parse_sensor2(
            r#"{"returnValue":true,"gameRotationVector":{"w":5,"x":0,"y":0,"z":0}}"#,
            0,
        );
        assert!(matches!(r, Reply::Error(MrcuError::Malformed(_))));
    }

    #[test]
    fn interval_and_state() {
        assert_eq!(
            parse_interval(r#"{"minInterval":10,"returnValue":true,"interval":1000,"maxInterval":1000}"#),
            Ok((1000, 10, 1000))
        );
        assert_eq!(parse_state(r#"{"isAlive":false,"returnValue":true}"#), Ok(false));
        assert_eq!(choose_interval(10, 1000, 100.0), 10);
        assert_eq!(choose_interval(20, 1000, 100.0), 20);
        assert_eq!(legacy_interval_code(100.0), 1);
        assert_eq!(legacy_interval_code(50.0), 2);
    }
}
