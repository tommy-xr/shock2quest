# FFmpeg ANR Fix

**Status**: Planning
**Priority**: High
**Created**: 2025-11-10

## Problem

FFmpeg integration is working successfully - AVI videos play correctly with proper rendering and interactivity. However, the app triggers an ANR (Application Not Responding) popup during video playback:

```
ANR in Window{...com.tommybuilds.shock2quest/android.app.NativeActivity}
Reason: Input dispatching timed out ... Waited 5002ms for KeyEvent
```

The ANR occurs because video decoding operations are blocking the main thread for more than 5 seconds, preventing input event processing.

## Root Cause Analysis

- **Main Thread Blocking**: FFmpeg video operations are running on the main/UI thread
- **Synchronous Operations**: Video loading, decoding, or frame processing is blocking
- **Input Event Timeout**: Android's watchdog triggers ANR when main thread is blocked >5s

## Solution Approach

Move video operations off the main thread using proper async/threading architecture.

## Implementation Plan

### Phase 1: Identify Current Architecture
- [ ] **Audit video code paths** - Find where FFmpeg operations occur in the codebase
- [ ] **Map thread usage** - Identify which thread handles video loading, decoding, rendering
- [ ] **Profile blocking operations** - Measure which FFmpeg calls take >5 seconds
- [ ] **Document current flow** - Create diagram of video processing pipeline

**Deliverables**:
- Architecture documentation showing current video processing flow
- Performance profile identifying blocking operations
- Thread usage analysis

### Phase 2: Design Threading Architecture
- [ ] **Design worker thread system** - Plan background threads for video operations
- [ ] **Define thread communication** - Design message passing between threads
- [ ] **Plan frame buffering** - Design queue system for decoded frames
- [ ] **Error handling strategy** - Plan how to handle async errors gracefully

**Deliverables**:
- Threading architecture design document
- Frame pipeline design with buffering strategy
- Error handling specification

### Phase 3: Implement Background Video Processing
- [ ] **Create video worker thread** - Implement dedicated thread for FFmpeg operations
- [ ] **Move file loading to background** - Make video file I/O async
- [ ] **Implement async decoding** - Move frame decoding off main thread
- [ ] **Add frame queue system** - Implement producer/consumer pattern for frames

**Deliverables**:
- Video worker thread implementation
- Async file loading system
- Frame queue/buffering system

### Phase 4: Update Main Thread Integration
- [ ] **Modify main thread to consume frames** - Update render loop to use queued frames
- [ ] **Implement non-blocking frame requests** - Ensure main thread never waits for decode
- [ ] **Add loading state handling** - Show progress indicators during async loading
- [ ] **Update input handling** - Ensure input remains responsive during video operations

**Deliverables**:
- Updated main thread video integration
- Non-blocking frame consumption
- Loading state UI

### Phase 5: Testing and Optimization
- [ ] **Test ANR resolution** - Verify no more "not responding" popups
- [ ] **Performance validation** - Ensure smooth video playback maintained
- [ ] **Memory usage analysis** - Check frame buffering doesn't cause memory issues
- [ ] **Edge case testing** - Test with various video sizes/formats
- [ ] **VR performance testing** - Ensure VR frame rates remain stable

**Deliverables**:
- ANR-free video playback
- Performance benchmarks
- Memory usage analysis
- Edge case test results

## Technical Considerations

### Threading Strategy
- **Video Worker Thread**: Dedicated thread for all FFmpeg operations
- **Frame Queue**: Lock-free circular buffer for decoded frames
- **Main Thread**: Only consumes pre-decoded frames, never blocks

### Performance Requirements
- **Frame Rate**: Maintain 72/90fps VR rendering during video playback
- **Latency**: Video start latency <2 seconds acceptable
- **Memory**: Frame buffer should not exceed 100MB
- **Responsiveness**: Input lag should remain <20ms

### Risk Mitigation
- **Threading Bugs**: Use well-tested synchronization primitives
- **Memory Leaks**: Implement RAII patterns for frame management
- **Deadlocks**: Avoid circular dependencies in thread communication
- **Performance Regression**: Benchmark before/after implementation

## Success Criteria

1. **ANR Eliminated**: No "app not responding" popups during video playback
2. **Responsiveness Maintained**: Input events processed <20ms during video
3. **Playback Quality**: Video rendering quality unchanged
4. **VR Performance**: Frame rate remains stable during video playback
5. **Reliability**: No crashes or deadlocks in video subsystem

## Future Enhancements

- **Multiple Video Support**: Support for multiple simultaneous video streams
- **Hardware Acceleration**: Investigate Android MediaCodec integration
- **Streaming Support**: Add support for network video streaming
- **Advanced Codecs**: Support for modern codecs (H.265, AV1)

## Resources

### Code Locations
- FFmpeg integration: `shock2vr/src/` (TBD - needs identification)
- Engine video: `engine/src/` (TBD - needs identification)
- VR runtime: `runtimes/oculus_runtime/src/` (TBD - needs identification)

### Reference Documentation
- [Android ANR Documentation](https://developer.android.com/topic/performance/vitals/anr)
- [FFmpeg Threading Guide](https://ffmpeg.org/doxygen/trunk/group__lavc__threading.html)
- [VR Performance Best Practices](https://developer.oculus.com/documentation/native/pc/dg-performance/)

## Notes

- Current FFmpeg integration is working correctly for video decode/playback
- Issue is purely about threading, not compatibility or functionality
- Solution should not impact existing desktop runtime
- Consider Quest hardware constraints when implementing threading