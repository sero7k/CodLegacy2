// Stand-in for the few UnityEngine members Klattersynth uses, so REPO's own
// Klattersynth.dll can make its speech samples outside Unity. The clip it
// creates keeps its reader; nothing is played here.
using System;
[assembly: System.Reflection.AssemblyVersion("0.0.0.0")]
namespace UnityEngine {
  public enum RuntimePlatform { OSXEditor = 0, OSXPlayer = 1, WindowsPlayer = 2, WindowsEditor = 7 }
  public class Object {
    public static bool operator ==(Object a, Object b) { return ReferenceEquals(a, b); }
    public static bool operator !=(Object a, Object b) { return !ReferenceEquals(a, b); }
    public override bool Equals(object o) { return ReferenceEquals(this, o); }
    public override int GetHashCode() { return base.GetHashCode(); }
  }
  public static class Mathf {
    public static float Sin(float f) { return (float)Math.Sin(f); }
    public static float Cos(float f) { return (float)Math.Cos(f); }
    public static float Exp(float f) { return (float)Math.Exp(f); }
    public static float Abs(float f) { return Math.Abs(f); }
    public static int Min(int a, int b) { return a < b ? a : b; }
    public static int Clamp(int v, int lo, int hi) { return v < lo ? lo : (v > hi ? hi : v); }
    public static float Clamp(float v, float lo, float hi) { return v < lo ? lo : (v > hi ? hi : v); }
    public static float Lerp(float a, float b, float t) { t = Clamp(t, 0f, 1f); return a + (b - a) * t; }
    public static int ClosestPowerOfTwo(int v) {
      if (v <= 1) return 1;
      int lo = 1; while (lo * 2 <= v) lo *= 2;
      int hi = lo * 2; return (v - lo) < (hi - v) ? lo : hi;
    }
  }
  public static class Application {
    public static bool isEditor { get { return false; } }
    public static RuntimePlatform platform { get { return RuntimePlatform.WindowsPlayer; } }
  }
  public static class Debug {
    public static string LastError;
    public static void LogError(object m) { LastError = m == null ? "" : m.ToString(); }
  }
  public static class AudioSettings { public static int outputSampleRate { get { return 48000; } } }
  public sealed class AudioClip : Object {
    public delegate void PCMReaderCallback(float[] data);
    public delegate void PCMSetPositionCallback(int position);
    public string name; public int lengthSamples, channels, frequency; public bool stream;
    public PCMReaderCallback reader; public PCMSetPositionCallback setPosition;
    public static AudioClip Create(string name, int lengthSamples, int channels, int frequency, bool stream, PCMReaderCallback pcmreadercallback) {
      return Create(name, lengthSamples, channels, frequency, stream, pcmreadercallback, null);
    }
    public static AudioClip Create(string name, int lengthSamples, int channels, int frequency, bool stream, PCMReaderCallback pcmreadercallback, PCMSetPositionCallback pcmsetpositioncallback) {
      return new AudioClip { name = name, lengthSamples = lengthSamples, channels = channels, frequency = frequency, stream = stream, reader = pcmreadercallback, setPosition = pcmsetpositioncallback };
    }
    /// All its samples, read as Unity reads a clip it is given.
    public float[] ReadAll() {
      var all = new float[lengthSamples * Math.Max(1, channels)];
      if (setPosition != null) setPosition(0);
      const int chunk = 4096;
      for (int at = 0; at < all.Length; at += chunk) {
        var part = new float[Math.Min(chunk, all.Length - at)];
        if (reader != null) reader(part);
        Array.Copy(part, 0, all, at, part.Length);
      }
      return all;
    }
  }
  public sealed class AudioSource : Object {
    public AudioClip clip { get; set; }
    public bool loop { get; set; }
    public bool isPlaying { get { return false; } }
    public int timeSamples { get { return 0; } }
    public float time { get { return 0f; } }
    public void Play() { }
    public void Stop() { }
    public void GetOutputData(float[] samples, int channel) { Array.Clear(samples, 0, samples.Length); }
  }
}
