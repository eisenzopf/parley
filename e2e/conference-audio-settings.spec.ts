import { test, expect } from '@playwright/test';

test('audio settings select the monitor mic and speaker, show sound, and stop the check without placing a call', async ({ page }) => {
  await page.addInitScript(() => {
    const probe = (window as any).audioProbe = { inputs: [] as any[], outputs: [] as string[], streams: [] as MediaStream[], calls: 0 };
    const devices = [
      { kind: 'audioinput', deviceId: 'default', label: 'Default — Mac microphone' },
      { kind: 'audioinput', deviceId: 'mac-mic', label: 'Mac microphone' },
      { kind: 'audioinput', deviceId: 'display-mic', label: 'Studio Display Microphone' },
      { kind: 'audiooutput', deviceId: 'default', label: 'Default — Mac speakers' },
      { kind: 'audiooutput', deviceId: 'display-speaker', label: 'Studio Display Speakers' },
    ];
    navigator.mediaDevices.enumerateDevices = async () => devices as MediaDeviceInfo[];
    navigator.mediaDevices.getUserMedia = async constraints => {
      probe.inputs.push(constraints);
      const context = new AudioContext(); await context.resume();
      const tone = context.createOscillator(); tone.frequency.value = 440;
      const gain = context.createGain(); gain.gain.value = 0.3;
      const destination = context.createMediaStreamDestination(); tone.connect(gain).connect(destination); tone.start();
      const track = destination.stream.getAudioTracks()[0];
      Object.defineProperty(track, 'label', { value: 'Studio Display Microphone' });
      track.addEventListener('ended', () => context.close());
      probe.streams.push(destination.stream); return destination.stream;
    };
    HTMLMediaElement.prototype.setSinkId = async id => { probe.outputs.push(id); };
    (window as any).RTCPeerConnection = class { constructor() { probe.calls++; throw new Error('Mic check must not open a call'); } };
  });
  await page.goto('/conference/?manual=1');
  await expect(page.getByLabel('Microphone', { exact: true })).toBeVisible();
  expect(await page.evaluate(() => (window as any).audioProbe.inputs.length)).toBe(0);
  await page.getByLabel('Microphone', { exact: true }).selectOption('display-mic');
  await page.getByRole('button', { name: 'Check microphone', exact: true }).click();
  await expect(page.locator('#mic-status')).toContainText('Studio Display Microphone · receiving sound');
  expect(await page.locator('#mic-level').evaluate((el: HTMLMeterElement) => el.value)).toBeGreaterThan(0);
  const input = await page.evaluate(() => (window as any).audioProbe.inputs[0]);
  expect(input.audio.deviceId).toEqual({ exact: 'display-mic' }); expect(input.audio.echoCancellation).toBe(true);
  await page.getByRole('button', { name: 'Stop mic check', exact: true }).click();
  await expect(page.locator('#mic-status')).toHaveText('Microphone check stopped.');
  expect(await page.evaluate(() => (window as any).audioProbe.streams[0].getTracks()[0].readyState)).toBe('ended');
  await page.getByLabel('Speaker', { exact: true }).selectOption('display-speaker');
  await page.getByRole('button', { name: 'Test speaker', exact: true }).click();
  await expect(page.locator('#audio-status')).toContainText('Playing a short tone');
  expect(await page.evaluate(() => (window as any).audioProbe.outputs)).toContain('display-speaker');
  expect(await page.evaluate(() => (window as any).audioProbe.calls)).toBe(0);
  await page.reload();
  await expect(page.getByLabel('Microphone', { exact: true })).toHaveValue('display-mic');
  await expect(page.getByLabel('Speaker', { exact: true })).toHaveValue('display-speaker');
  expect(await page.evaluate(() => (window as any).audioProbe.inputs.length)).toBe(0);
});
