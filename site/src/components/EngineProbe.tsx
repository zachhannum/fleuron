import { useEffect, useState } from 'react';

/**
 * The island seam, with nothing mounted on it.
 *
 * Nothing here talks to the engine yet. It exists so that the React
 * runtime, the client directive and the hydration boundary are known
 * to work on a docs page before a demo depends on them.
 */
export default function EngineProbe() {
  const [hydrated, setHydrated] = useState(false);

  useEffect(() => {
    setHydrated(true);
  }, []);

  return (
    <div className="probe">
      <p className="probe-state">
        <span aria-hidden="true">{hydrated ? '●' : '○'}</span>{' '}
        {hydrated
          ? 'Island hydrated. React is running on this page.'
          : 'Server-rendered. This island has not hydrated yet.'}
      </p>
      <p className="probe-note">
        A demo that runs the engine mounts here, behind the same
        <code> client:visible</code> boundary, once the WebAssembly bindings
        and the SVG painter have landed.
      </p>
    </div>
  );
}
