using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Threading;
using System.Web.Script.Serialization;
using Microsoft.CognitiveServices.Speech;
using Microsoft.CognitiveServices.Speech.Audio;

namespace WindowsLiveCaptionsSttHelper
{
    internal static class Program
    {
        private static readonly JavaScriptSerializer Serializer = new JavaScriptSerializer();

        public static int Main(string[] args)
        {
            try
            {
                NativeDllSearch.Configure();
                if (args.Length == 0)
                {
                    Console.Error.WriteLine("missing command");
                    return 2;
                }

                var command = args[0];
                if (string.Equals(command, "list-models", StringComparison.OrdinalIgnoreCase))
                {
                    return ListModels(ParseOptions(args.Skip(1).ToArray()));
                }
                if (string.Equals(command, "recognize-file", StringComparison.OrdinalIgnoreCase))
                {
                    return RecognizeFile(ParseOptions(args.Skip(1).ToArray()));
                }
                if (string.Equals(command, "recognize-mic", StringComparison.OrdinalIgnoreCase))
                {
                    return RecognizeMic(ParseOptions(args.Skip(1).ToArray()));
                }

                Console.Error.WriteLine("unsupported command");
                return 2;
            }
            catch (Exception ex)
            {
                Console.Error.WriteLine(ex.ToString());
                return 1;
            }
        }

        private static int ListModels(Dictionary<string, string> options)
        {
            var modelRoot = GetModelRoot(options);
            using (PackageGraph.ForSpeechPackage(modelRoot))
            {
            var config = EmbeddedSpeechConfig.FromPaths(new[] { modelRoot });
            var models = config.GetSpeechRecognitionModels().Select(model => new Dictionary<string, object>
            {
                { "name", model.Name },
                { "path", model.Path },
                { "version", model.Version },
                { "locales", model.Locales },
            }).ToArray();
            WriteJson(new Dictionary<string, object>
            {
                { "model_root", modelRoot },
                { "models", models },
            });
            }
            return 0;
        }

        private static int RecognizeFile(Dictionary<string, string> options)
        {
            var audioPath = Require(options, "audio");
            if (!File.Exists(audioPath))
            {
                throw new FileNotFoundException("audio file not found", audioPath);
            }

            var modelRoot = GetModelRoot(options);
            using (PackageGraph.ForSpeechPackage(modelRoot))
            {
            var config = EmbeddedSpeechConfig.FromPaths(new[] { modelRoot });
            config.SpeechRecognitionOutputFormat = OutputFormat.Detailed;

            var models = config.GetSpeechRecognitionModels();
            if (models.Count == 0)
            {
                throw new InvalidOperationException("No embedded speech recognition models found under " + modelRoot);
            }

            var requestedModel = GetOption(options, "model");
            var model = string.IsNullOrWhiteSpace(requestedModel)
                ? models[0]
                : models.FirstOrDefault(item => string.Equals(item.Name, requestedModel, StringComparison.OrdinalIgnoreCase));
            if (model == null)
            {
                throw new InvalidOperationException("Speech recognition model not found: " + requestedModel);
            }

            var license = GetOption(options, "license");
            if (string.IsNullOrWhiteSpace(license))
            {
                license = ReadLicense(modelRoot, GetOption(options, "license-mode"));
            }
            config.SetSpeechRecognitionModel(model.Name, license);
            ApplyRecognitionProperties(config, false, GetOption(options, "profanity"));
            using (var audio = AudioConfig.FromWavFileInput(audioPath))
            using (var recognizer = new SpeechRecognizer(config, audio))
            {
                var done = new ManualResetEventSlim(false);
                var texts = new List<string>();
                string cancelReason = string.Empty;
                recognizer.Recognized += (sender, evt) =>
                {
                    if (!string.IsNullOrWhiteSpace(evt.Result.Text))
                    {
                        texts.Add(evt.Result.Text);
                    }
                };
                recognizer.Canceled += (sender, evt) =>
                {
                    cancelReason = evt.Reason.ToString();
                    done.Set();
                };
                recognizer.SessionStopped += (sender, evt) => done.Set();
                recognizer.StartContinuousRecognitionAsync().GetAwaiter().GetResult();
                done.Wait(TimeSpan.FromSeconds(120));
                recognizer.StopContinuousRecognitionAsync().GetAwaiter().GetResult();
                var text = string.Join(" ", texts.ToArray()).Trim();
                WriteJson(new Dictionary<string, object>
                {
                    { "model_root", modelRoot },
                    { "model", model.Name },
                    { "reason", string.IsNullOrWhiteSpace(text) ? cancelReason : "RecognizedSpeech" },
                    { "text", text },
                    { "segments", texts.ToArray() },
                });
            }
            }
            return 0;
        }

        private static int RecognizeMic(Dictionary<string, string> options)
        {
            var secondsRaw = GetOption(options, "seconds");
            int seconds;
            var hasLimit = int.TryParse(secondsRaw, out seconds) && seconds > 0;
            var stop = new ManualResetEventSlim(false);
            ConsoleCancelEventHandler cancelHandler = (sender, evt) =>
            {
                evt.Cancel = true;
                stop.Set();
            };
            Console.CancelKeyPress += cancelHandler;
            using (var session = CreateRecognitionSession(options, null))
            {
                try
                {
                    session.Recognizer.StartContinuousRecognitionAsync().GetAwaiter().GetResult();
                    if (hasLimit)
                    {
                        stop.Wait(TimeSpan.FromSeconds(seconds));
                    }
                    else
                    {
                        stop.Wait();
                    }
                    session.Recognizer.StopContinuousRecognitionAsync().GetAwaiter().GetResult();
                }
                finally
                {
                    Console.CancelKeyPress -= cancelHandler;
                }
            }
            return 0;
        }

        private sealed class RecognitionSession : IDisposable
        {
            public SpeechRecognizer Recognizer;
            public AudioConfig Audio;
            public PackageGraph Graph;

            public void Dispose()
            {
                if (Recognizer != null) Recognizer.Dispose();
                if (Audio != null) Audio.Dispose();
                if (Graph != null) Graph.Dispose();
            }
        }

        private static RecognitionSession CreateRecognitionSession(Dictionary<string, string> options, string audioPath)
        {
            var modelRoot = GetModelRoot(options);
            var graph = PackageGraph.ForSpeechPackage(modelRoot);
            var config = EmbeddedSpeechConfig.FromPaths(new[] { modelRoot });
            config.SpeechRecognitionOutputFormat = OutputFormat.Detailed;
            var models = config.GetSpeechRecognitionModels();
            if (models.Count == 0)
            {
                throw new InvalidOperationException("No embedded speech recognition models found under " + modelRoot);
            }
            var requestedModel = GetOption(options, "model");
            var model = string.IsNullOrWhiteSpace(requestedModel)
                ? models[0]
                : models.FirstOrDefault(item => string.Equals(item.Name, requestedModel, StringComparison.OrdinalIgnoreCase));
            if (model == null)
            {
                throw new InvalidOperationException("Speech recognition model not found: " + requestedModel);
            }
            var license = GetOption(options, "license");
            if (string.IsNullOrWhiteSpace(license))
            {
                license = ReadLicense(modelRoot, GetOption(options, "license-mode"));
            }
            config.SetSpeechRecognitionModel(model.Name, license);
            ApplyRecognitionProperties(config, true, GetOption(options, "profanity"));
            var audio = string.IsNullOrWhiteSpace(audioPath)
                ? AudioConfig.FromDefaultMicrophoneInput()
                : AudioConfig.FromWavFileInput(audioPath);
            var recognizer = new SpeechRecognizer(config, audio);
            var jsonOutput = string.Equals(GetOption(options, "json"), "true", StringComparison.OrdinalIgnoreCase);
            var finalOnly = string.Equals(GetOption(options, "final-only"), "true", StringComparison.OrdinalIgnoreCase);
            var lastPartialLength = 0;
            object consoleLock = new object();
            if (!finalOnly)
            {
                recognizer.Recognizing += (sender, evt) =>
                {
                    if (string.IsNullOrWhiteSpace(evt.Result.Text))
                    {
                        return;
                    }
                    lock (consoleLock)
                    {
                        if (jsonOutput)
                        {
                            WriteJson(new Dictionary<string, object>
                            {
                                { "event", "partial" },
                                { "text", evt.Result.Text },
                                { "reason", evt.Result.Reason.ToString() },
                            });
                        }
                        else
                        {
                            WritePartialLine(evt.Result.Text, ref lastPartialLength);
                        }
                    }
                };
            }
            recognizer.Recognized += (sender, evt) =>
            {
                if (!string.IsNullOrWhiteSpace(evt.Result.Text))
                {
                    lock (consoleLock)
                    {
                        if (jsonOutput)
                        {
                            WriteJson(new Dictionary<string, object>
                            {
                                { "event", "final" },
                                { "text", evt.Result.Text },
                                { "reason", evt.Result.Reason.ToString() },
                            });
                        }
                        else
                        {
                            CommitFinalLine(evt.Result.Text, ref lastPartialLength);
                        }
                        Console.Out.Flush();
                    }
                }
            };
            return new RecognitionSession
            {
                Recognizer = recognizer,
                Audio = audio,
                Graph = graph,
            };
        }

        private static Dictionary<string, string> ParseOptions(string[] args)
        {
            var options = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
            for (var i = 0; i < args.Length; i++)
            {
                if (!args[i].StartsWith("--", StringComparison.Ordinal))
                {
                    continue;
                }
                var key = args[i].Substring(2);
                if (i + 1 >= args.Length || args[i + 1].StartsWith("--", StringComparison.Ordinal))
                {
                    options[key] = "true";
                }
                else
                {
                    options[key] = args[++i];
                }
            }
            return options;
        }

        private static string GetModelRoot(Dictionary<string, string> options)
        {
            var explicitRoot = GetOption(options, "model-root");
            if (!string.IsNullOrWhiteSpace(explicitRoot))
            {
                return explicitRoot;
            }
            var roots = Directory.GetDirectories(
                @"C:\Program Files\WindowsApps",
                "MicrosoftWindows.Speech.*",
                SearchOption.TopDirectoryOnly);
            var enUs = roots.FirstOrDefault(path => Path.GetFileName(path).IndexOf("en-US", StringComparison.OrdinalIgnoreCase) >= 0);
            var root = enUs ?? roots.FirstOrDefault();
            if (string.IsNullOrWhiteSpace(root))
            {
                throw new DirectoryNotFoundException("No MicrosoftWindows.Speech.* package found under WindowsApps.");
            }
            return root;
        }

        private static string ReadLicense(string modelRoot, string mode)
        {
            var binaryLicense = ExtractRecognizerLicense(string.IsNullOrWhiteSpace(mode) ? "legal" : mode);
            if (!string.IsNullOrWhiteSpace(binaryLicense))
            {
                return binaryLicense;
            }

            var versionPath = Path.Combine(modelRoot, "version.txt");
            if (File.Exists(versionPath))
            {
                var lines = File.ReadAllLines(versionPath);
                if (lines.Length > 1)
                {
                    return lines[1].Trim();
                }
            }
            return string.Empty;
        }

        private static string ExtractRecognizerLicense(string mode)
        {
            var candidates = new[]
            {
                @"C:\Windows\SystemApps\MicrosoftWindows.Client.Core_cw5n1h2txyewy\SpeechRecognizer.dll",
                @"C:\Windows\SystemApps\MicrosoftWindows.Client.Core_cw5n1h2txyewy\LiveCaptionsBackendDll.dll",
            };
            foreach (var candidate in candidates)
            {
                if (!File.Exists(candidate))
                {
                    continue;
                }
                var bytes = File.ReadAllBytes(candidate);
                var marker = string.Equals(mode, "legal", StringComparison.OrdinalIgnoreCase)
                    ? "This model and the software may not be used"
                    : "Key:XUw7C0rc";
                var license = ExtractContainingNullTerminatedString(bytes, System.Text.Encoding.UTF8.GetBytes(marker), System.Text.Encoding.UTF8, 1);
                if (!string.IsNullOrWhiteSpace(license))
                {
                    return license;
                }
                license = ExtractContainingNullTerminatedString(bytes, System.Text.Encoding.Unicode.GetBytes(marker), System.Text.Encoding.Unicode, 2);
                if (!string.IsNullOrWhiteSpace(license))
                {
                    return license;
                }
            }
            return string.Empty;
        }

        private static string ExtractContainingNullTerminatedString(byte[] bytes, byte[] markerBytes, System.Text.Encoding encoding, int terminatorWidth)
        {
            var markerIndex = IndexOf(bytes, markerBytes);
            if (markerIndex < 0)
            {
                return null;
            }

            var start = markerIndex;
            while (start - terminatorWidth >= 0 && !IsTerminatorAt(bytes, start - terminatorWidth, terminatorWidth))
            {
                start -= terminatorWidth;
            }

            var end = markerIndex + markerBytes.Length;
            while (end + terminatorWidth - 1 < bytes.Length)
            {
                if (IsTerminatorAt(bytes, end, terminatorWidth))
                {
                    break;
                }
                end += terminatorWidth;
            }

            return end > start ? encoding.GetString(bytes, start, end - start) : null;
        }

        private static int IndexOf(byte[] bytes, byte[] pattern)
        {
            for (var i = 0; i <= bytes.Length - pattern.Length; i++)
            {
                var match = true;
                for (var j = 0; j < pattern.Length; j++)
                {
                    if (bytes[i + j] != pattern[j])
                    {
                        match = false;
                        break;
                    }
                }
                if (match)
                {
                    return i;
                }
            }
            return -1;
        }

        private static bool IsTerminatorAt(byte[] bytes, int offset, int width)
        {
            for (var i = 0; i < width; i++)
            {
                if (bytes[offset + i] != 0)
                {
                    return false;
                }
            }
            return true;
        }

        private static Dictionary<string, string> ReadResultProperties(SpeechRecognitionResult result)
        {
            var names = new[]
            {
                "SpeechServiceResponse_JsonResult",
                "SpeechServiceResponse_RecognitionLatencyMs",
                "SpeechServiceResponse_RequestWordLevelTimestamps",
            };
            var values = new Dictionary<string, string>();
            foreach (var name in names)
            {
                var value = result.Properties.GetProperty(name);
                if (!string.IsNullOrWhiteSpace(value))
                {
                    values[name] = value;
                }
            }
            return values;
        }

        private static string Require(Dictionary<string, string> options, string name)
        {
            var value = GetOption(options, name);
            if (string.IsNullOrWhiteSpace(value))
            {
                throw new ArgumentException("missing --" + name);
            }
            return value;
        }

        private static string GetOption(Dictionary<string, string> options, string name)
        {
            string value;
            return options.TryGetValue(name, out value) ? value : string.Empty;
        }

        private static void WriteJson(object value)
        {
            Console.WriteLine(Serializer.Serialize(value));
        }

        private static void ApplyRecognitionProperties(EmbeddedSpeechConfig config, bool microphone, string profanity)
        {
            config.SetProperty("SpeechRecognition_SegmentationFlavor", "aggressive");
            config.SetProperty("SpeechRecognition_PunctuationMode", "explicit");
            config.SetProperty("SpeechRecognition_RequestPerformanceMetrics", "true");
            config.SetProperty("SpeechRecognition_RequestWordLevelCorrections", microphone ? "false" : "true");
            config.SetProperty(PropertyId.SpeechServiceResponse_RequestProfanityFilterTrueFalse, "false");
            config.SetProfanity(ParseProfanity(profanity));
        }

        private static ProfanityOption ParseProfanity(string value)
        {
            if (string.IsNullOrWhiteSpace(value))
            {
                return ProfanityOption.Raw;
            }
            if (string.Equals(value, "masked", StringComparison.OrdinalIgnoreCase))
            {
                return ProfanityOption.Masked;
            }
            if (string.Equals(value, "removed", StringComparison.OrdinalIgnoreCase))
            {
                return ProfanityOption.Removed;
            }
            return ProfanityOption.Raw;
        }

        private static void WritePartialLine(string text, ref int lastPartialLength)
        {
            var padding = Math.Max(0, lastPartialLength - text.Length);
            Console.Write("\r" + text + new string(' ', padding));
            lastPartialLength = text.Length;
            Console.Out.Flush();
        }

        private static void CommitFinalLine(string text, ref int lastPartialLength)
        {
            if (lastPartialLength > 0)
            {
                Console.Write("\r" + new string(' ', lastPartialLength) + "\r");
                lastPartialLength = 0;
            }
            Console.WriteLine(text);
        }

        private sealed class PackageGraph : IDisposable
        {
            private IntPtr context;
            private string dependencyId;

            public static PackageGraph ForSpeechPackage(string modelRoot)
            {
                if (modelRoot.IndexOf(@"\WindowsApps\MicrosoftWindows.Speech.", StringComparison.OrdinalIgnoreCase) < 0)
                {
                    return new PackageGraph();
                }
                var family = Path.GetFileName(modelRoot);
                var marker = family.IndexOf("_1.", StringComparison.Ordinal);
                if (marker > 0)
                {
                    family = family.Substring(0, marker) + "_cw5n1h2txyewy";
                }
                var graph = new PackageGraph();
                graph.Add(family);
                return graph;
            }

            private void Add(string packageFamilyName)
            {
                PACKAGE_VERSION version = new PACKAGE_VERSION();
                version.Version = 0;
                IntPtr idPtr;
                var createHr = TryCreatePackageDependency(
                    IntPtr.Zero,
                    packageFamilyName,
                    version,
                    4,
                    0,
                    null,
                    0,
                    out idPtr);
                if (createHr != 0)
                {
                    System.Runtime.InteropServices.Marshal.ThrowExceptionForHR(createHr);
                }
                dependencyId = System.Runtime.InteropServices.Marshal.PtrToStringUni(idPtr);
                IntPtr fullName;
                var addHr = AddPackageDependency(dependencyId, 0, 0, out context, out fullName);
                if (addHr != 0)
                {
                    System.Runtime.InteropServices.Marshal.ThrowExceptionForHR(addHr);
                }
            }

            public void Dispose()
            {
                if (context != IntPtr.Zero)
                {
                    RemovePackageDependency(context);
                    context = IntPtr.Zero;
                }
                if (!string.IsNullOrWhiteSpace(dependencyId))
                {
                    DeletePackageDependency(dependencyId);
                    dependencyId = null;
                }
            }

            [System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Explicit)]
            private struct PACKAGE_VERSION
            {
                [System.Runtime.InteropServices.FieldOffset(0)]
                public ulong Version;
            }

            [System.Runtime.InteropServices.DllImport("kernelbase.dll", CharSet = System.Runtime.InteropServices.CharSet.Unicode, ExactSpelling = true)]
            private static extern int TryCreatePackageDependency(IntPtr user, string packageFamilyName, PACKAGE_VERSION minVersion, uint architectures, uint lifetimeKind, string lifetimeArtifact, uint options, out IntPtr packageDependencyId);

            [System.Runtime.InteropServices.DllImport("kernelbase.dll", CharSet = System.Runtime.InteropServices.CharSet.Unicode, ExactSpelling = true)]
            private static extern int AddPackageDependency(string packageDependencyId, int rank, uint options, out IntPtr packageDependencyContext, out IntPtr packageFullName);

            [System.Runtime.InteropServices.DllImport("kernelbase.dll", ExactSpelling = true)]
            private static extern void RemovePackageDependency(IntPtr packageDependencyContext);

            [System.Runtime.InteropServices.DllImport("kernelbase.dll", CharSet = System.Runtime.InteropServices.CharSet.Unicode, ExactSpelling = true)]
            private static extern int DeletePackageDependency(string packageDependencyId);
        }

        private static class NativeDllSearch
        {
            private const uint LOAD_LIBRARY_SEARCH_DEFAULT_DIRS = 0x00001000;
            private const uint LOAD_LIBRARY_SEARCH_USER_DIRS = 0x00000400;

            public static void Configure()
            {
                SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_DEFAULT_DIRS | LOAD_LIBRARY_SEARCH_USER_DIRS);
                AddIfExists(AppDomain.CurrentDomain.BaseDirectory);
                AddIfExists(@"C:\Windows\SystemApps\MicrosoftWindows.Client.Core_cw5n1h2txyewy\LiveCaptions");
                AddIfExists(@"C:\Windows\SystemApps\MicrosoftWindows.Client.Core_cw5n1h2txyewy");
            }

            private static void AddIfExists(string path)
            {
                if (Directory.Exists(path))
                {
                    AddDllDirectory(path);
                }
            }

            [System.Runtime.InteropServices.DllImport("kernel32.dll", SetLastError = true)]
            private static extern bool SetDefaultDllDirectories(uint directoryFlags);

            [System.Runtime.InteropServices.DllImport("kernel32.dll", CharSet = System.Runtime.InteropServices.CharSet.Unicode, SetLastError = true)]
            private static extern IntPtr AddDllDirectory(string newDirectory);
        }
    }
}
