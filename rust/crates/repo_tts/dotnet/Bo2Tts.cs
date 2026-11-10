// bo2mp: speaks a chat line with REPO's Klattersynth voice. REPO's TTSVoice
// schedules a line word by word, each word one clip played straight after
// the last; this makes those clips and joins them, as 11025 Hz samples.
using System; using System.Text; using System.Collections.Generic;
using Strobotnik.Klattersynth;
public static class Bo2Tts {
  static readonly Dictionary<long, SpeechSynth> synths = new Dictionary<long, SpeechSynth>();
  public const int SampleRate = 11025;
  public static string LastError() { return UnityEngine.Debug.LastError ?? ""; }
  public static float[] Speak(string text, int baseFrequency, bool whisper, int msPerSpeechFrame, int flutter, float flutterSpeed) {
    // (No string formatting: the key is numbers only.)
    long key = ((long)msPerSpeechFrame << 40) ^ ((long)flutter << 20) ^ (long)(flutterSpeed * 1000f);
    SpeechSynth synth;
    if (!synths.TryGetValue(key, out synth)) {
      synth = new SpeechSynth();
      synth.init(new UnityEngine.AudioSource(), false, SampleRate, msPerSpeechFrame, flutter, flutterSpeed);
      synths[key] = synth;
    }
    var voicing = whisper ? SpeechSynth.VoicingSource.whisper : SpeechSynth.VoicingSource.natural;
    var all = new List<float>();
    foreach (var word in text.Split(new[] { ' ' }, StringSplitOptions.RemoveEmptyEntries)) {
      var clip = synth.pregenerate(new StringBuilder(word), baseFrequency, voicing, false, false);
      if (clip == null || clip.pregenAudio == null) continue;
      all.AddRange(clip.pregenAudio.ReadAll());
    }
    return all.ToArray();
  }
}
