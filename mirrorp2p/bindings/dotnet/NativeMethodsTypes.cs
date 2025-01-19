using System.Runtime.InteropServices;

namespace SuperNeko.MirrorP2P {
    internal static unsafe partial class MirrorP2PNative {
        [StructLayout(LayoutKind.Sequential)]
        internal unsafe partial struct MirrorClient
        {
            fixed byte _unused[1];
        }

        [StructLayout(LayoutKind.Sequential)]
        internal unsafe partial struct NetworkContext
        {
            fixed byte _unused[1];
        }

        [StructLayout(LayoutKind.Sequential)]
        internal unsafe partial struct FfiResult
        {
            fixed int result[1];
        }
    }
}
