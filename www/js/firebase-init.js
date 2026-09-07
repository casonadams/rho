// Firebase Web SDK initialization
import { initializeApp } from "https://www.gstatic.com/firebasejs/11.3.0/firebase-app.js";
import { getAnalytics, isSupported } from "https://www.gstatic.com/firebasejs/11.3.0/firebase-analytics.js";

const firebaseConfig = {
  apiKey: "AIzaSyB_ldElUloHJQALks2kAhiY99igt4mtxEk",
  authDomain: "getrho.firebaseapp.com",
  projectId: "getrho",
  storageBucket: "getrho.firebasestorage.app",
  messagingSenderId: "200904538809",
  appId: "1:200904538809:web:579eb4d5f49c78b4f57968"
};

export const app = initializeApp(firebaseConfig);

// Initialize analytics if supported in current browser environment
export let analytics = null;
isSupported().then((supported) => {
  if (supported) {
    analytics = getAnalytics(app);
  }
}).catch(() => {
  // Silent fallback for environments without storage access / tracking protection
});
