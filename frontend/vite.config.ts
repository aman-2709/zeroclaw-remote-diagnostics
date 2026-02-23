import { sveltekit } from '@sveltejs/kit/vite';
import tailwindcss from '@tailwindcss/vite';
import { defineConfig, loadEnv } from 'vite';

export default defineConfig(({ mode }) => {
	const env = loadEnv(mode, '.', '');
	const apiTarget = env.API_URL || 'http://localhost:3000';

	return {
		plugins: [tailwindcss(), sveltekit()],
		server: {
			proxy: {
				'/api': { target: apiTarget, ws: true },
				'/health': apiTarget
			}
		}
	};
});
